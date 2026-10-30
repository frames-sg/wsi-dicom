//! A strict, bounded multipart/related reader (PS3.18 8.6.1.2.1, RFC 2387,
//! RFC 2046 5.1).
//!
//! It reads what the origin server sent, not what a lenient client would
//! recover: framing that departs from the 8.6.1.2.1 syntax is reported with
//! the part it occurs in, and a part whose Content-Length disagrees with its
//! delimiters is reported rather than silently re-cut.

use super::EDITION;

/// Parts beyond this count are not read.
const MAX_PARTS: usize = 100_000;
/// Header section bytes of one part beyond which it is malformed.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// One body part: its header fields (names lowercase) and payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Part<'a> {
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: &'a [u8],
}

impl Part<'_> {
    /// The first value of header field `name` (lowercase).
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
    }
}

/// The parts read, and each departure from the multipart syntax.
#[derive(Debug, Default)]
pub(crate) struct Multipart<'a> {
    pub(crate) parts: Vec<Part<'a>>,
    pub(crate) violations: Vec<String>,
}

fn cite() -> String {
    format!("PS3.18 {EDITION} 8.6.1.2.1")
}

/// PS3.18 8.6.1.2.1: `boundary = 0*69(bchar / SP) bchar`.
pub(crate) fn boundary_violation(boundary: &str) -> Option<String> {
    let bchar = |byte: u8| byte.is_ascii_alphanumeric() || b"'()+_,-./:=?".contains(&byte);
    if boundary.is_empty() || boundary.len() > 70 {
        return Some(format!(
            "the boundary is {} characters; it has 1 to 70 ({})",
            boundary.len(),
            cite()
        ));
    }
    if !boundary.bytes().all(|byte| bchar(byte) || byte == b' ') {
        return Some(format!(
            "the {}-character boundary holds a character outside bchar ({})",
            boundary.chars().count(),
            cite()
        ));
    }
    boundary.ends_with(' ').then(|| {
        format!(
            "the {}-character boundary ends with a space; its last character is a bchar ({})",
            boundary.chars().count(),
            cite()
        )
    })
}

/// Reads `body` as `multipart-payload = 1*(DASH boundary CRLF part CRLF)
/// DASH boundary DASH`.
pub(crate) fn parse<'a>(body: &'a [u8], boundary: &str) -> Multipart<'a> {
    let mut result = Multipart::default();
    if let Some(violation) = boundary_violation(boundary) {
        result.violations.push(violation);
    }
    let dash_boundary = format!("--{boundary}").into_bytes();
    let delimiter = format!("\r\n--{boundary}").into_bytes();
    let Some(first) = find(body, &dash_boundary, 0) else {
        result.violations.push(format!(
            "the payload holds no delimiter for its {}-character boundary ({})",
            boundary.chars().count(),
            cite()
        ));
        return result;
    };
    if first > 0 {
        result.violations.push(format!(
            "{first} bytes precede the first boundary; the payload begins with its boundary delimiter ({})",
            cite()
        ));
    }
    let mut position = first + dash_boundary.len();
    loop {
        if body[position..].starts_with(b"--") {
            let end = position + 2;
            let epilogue = &body[end..];
            if !epilogue.is_empty() && epilogue != b"\r\n" {
                result.violations.push(format!(
                    "{} bytes follow the close delimiter ({})",
                    epilogue.len(),
                    cite()
                ));
            }
            break;
        }
        if !body[position..].starts_with(b"\r\n") {
            result.violations.push(format!(
                "part {}: the boundary is not followed by CRLF ({})",
                result.parts.len() + 1,
                cite()
            ));
            match skip_to_line_end(body, position) {
                Some(next) => position = next,
                None => break,
            }
        } else {
            position += 2;
        }
        if result.parts.len() == MAX_PARTS {
            result.violations.push(format!(
                "more than {MAX_PARTS} parts; the rest are not read"
            ));
            break;
        }
        match read_part(body, position, &delimiter, result.parts.len() + 1) {
            Ok((part, next, mut violations)) => {
                result.parts.push(part);
                result.violations.append(&mut violations);
                position = next;
            }
            Err(violation) => {
                result.violations.push(violation);
                break;
            }
        }
    }
    if result.parts.is_empty() {
        result.violations.push(format!(
            "the payload has no part; a multipart payload holds at least one ({})",
            cite()
        ));
    }
    result
}

/// One part from `start`: header fields, an empty line, the payload up to
/// the next delimiter. Returns the part, where the delimiter's boundary ends
/// and the part's violations.
fn read_part<'a>(
    body: &'a [u8],
    start: usize,
    delimiter: &[u8],
    number: usize,
) -> Result<(Part<'a>, usize, Vec<String>), String> {
    let mut violations = Vec::new();
    let (headers, payload_start) = if body[start..].starts_with(b"\r\n") {
        (Vec::new(), start + 2)
    } else {
        let end = find(body, b"\r\n\r\n", start)
            .filter(|end| end - start <= MAX_HEADER_BYTES)
            .ok_or_else(|| {
                format!(
                    "part {number}: no empty line ends the header fields within {MAX_HEADER_BYTES} bytes ({})",
                    cite()
                )
            })?;
        (
            header_fields(&body[start..end], number, &mut violations),
            end + 4,
        )
    };
    let declared = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map(|(_, value)| value.trim().parse::<usize>());
    let found = find(body, delimiter, payload_start).ok_or_else(|| {
        format!(
            "part {number}: no delimiter ends the part; the payload is truncated ({})",
            cite()
        )
    })?;
    let mut payload_end = found;
    match declared {
        Some(Ok(length)) if payload_start.checked_add(length) != Some(found) => {
            let at_length = payload_start.checked_add(length).filter(|end| {
                body.get(*end..)
                    .is_some_and(|rest| rest.starts_with(delimiter))
            });
            violations.push(format!(
                "part {number}: Content-Length {length} but the delimiter follows {} payload bytes ({})",
                found - payload_start,
                cite()
            ));
            if let Some(end) = at_length {
                payload_end = end;
            }
        }
        Some(Err(_)) => violations.push(format!(
            "part {number}: Content-Length is not a decimal byte count ({})",
            cite()
        )),
        _ => {}
    }
    let next = payload_end + delimiter.len();
    Ok((
        Part {
            headers,
            body: &body[payload_start..payload_end],
        },
        next,
        violations,
    ))
}

fn header_fields(
    bytes: &[u8],
    number: usize,
    violations: &mut Vec<String>,
) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(bytes);
    let mut fields = Vec::new();
    for line in text.split("\r\n") {
        if line.starts_with([' ', '\t']) {
            violations.push(format!(
                "part {number}: a {}-character header field line is folded, which RFC 7230 3.2.4 forbids",
                line.chars().count()
            ));
            continue;
        }
        match line.split_once(':') {
            Some((name, value))
                if !name.is_empty()
                    && name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
                    }) =>
            {
                fields.push((name.to_ascii_lowercase(), value.trim().to_string()));
            }
            _ => violations.push(format!(
                "part {number}: a {}-character line is not a header field ({})",
                line.chars().count(),
                cite()
            )),
        }
    }
    fields
}

fn skip_to_line_end(body: &[u8], from: usize) -> Option<usize> {
    find(body, b"\r\n", from).map(|end| end + 2)
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from > haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| from + offset)
}

#[cfg(test)]
mod tests {
    use super::{boundary_violation, parse};

    fn payload(parts: &[(&str, &[u8])], boundary: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for (headers, body) in parts {
            out.extend_from_slice(format!("--{boundary}\r\n{headers}\r\n\r\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(format!("--{boundary}--").as_bytes());
        out
    }

    #[test]
    fn conforming_payload_yields_parts_in_order() {
        let body = payload(
            &[
                ("Content-Type: image/jpeg\r\nContent-Length: 3", b"\x01\r\n"),
                ("Content-Type: image/jpeg\r\nContent-Length: 2", b"--"),
            ],
            "b1",
        );
        let read = parse(&body, "b1");
        assert!(read.violations.is_empty(), "{:?}", read.violations);
        assert_eq!(read.parts.len(), 2);
        assert_eq!(read.parts[0].body, b"\x01\r\n");
        assert_eq!(read.parts[1].header("content-type"), Some("image/jpeg"));
    }

    #[test]
    fn each_framing_defect_is_reported() {
        let good = payload(&[("Content-Type: a/b\r\nContent-Length: 2", b"ab")], "x");
        let mut preamble = b"\r\n".to_vec();
        preamble.extend_from_slice(&good);
        assert!(parse(&preamble, "x").violations[0].contains("precede the first boundary"));

        let mut epilogue = good.clone();
        epilogue.extend_from_slice(b"junk");
        assert!(parse(&epilogue, "x").violations[0].contains("follow the close delimiter"));

        let truncated = &good[..good.len() - 4];
        assert!(parse(truncated, "x")
            .violations
            .iter()
            .any(|v| v.contains("truncated")));

        let wrong_length = payload(&[("Content-Type: a/b\r\nContent-Length: 5", b"ab")], "x");
        assert!(parse(&wrong_length, "x").violations[0].contains("Content-Length 5"));

        let folded = payload(&[("Content-Type: a/b\r\n  folded", b"ab")], "x");
        assert!(parse(&folded, "x").violations[0].contains("folded"));

        assert!(parse(b"no parts here", "x").violations[0].contains("no delimiter"));
        assert!(boundary_violation("ends ").is_some());
        assert!(boundary_violation(&"a".repeat(71)).is_some());
        assert!(boundary_violation("ok'()+_,-./:=? x").is_none());

        const SENTINEL: &str = "PatientSentinel";
        let body = format!(
            "--{SENTINEL}\r\n{SENTINEL}\r\n folded-{SENTINEL}\r\n\r\nbody\r\n--{SENTINEL}--"
        );
        let report = parse(body.as_bytes(), SENTINEL).violations.join("\n");
        assert!(!report.contains(SENTINEL), "{report}");
        let report = boundary_violation(&format!("{SENTINEL}@"))
            .expect("the boundary contains a character outside bchar");
        assert!(!report.contains(SENTINEL), "{report}");
    }

    #[test]
    fn part_without_header_fields_is_read() {
        let read = parse(b"--x\r\n\r\nab\r\n--x--", "x");
        assert_eq!(read.parts.len(), 1);
        assert_eq!(read.parts[0].body, b"ab");
        assert!(read.parts[0].headers.is_empty());
    }
}
