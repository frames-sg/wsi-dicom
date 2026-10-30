//! A strict RFC 8259 JSON reader for DICOM JSON documents.
//!
//! General JSON libraries keep the last of duplicate member names, reorder
//! members and turn numbers into binary floating point, so the members' order
//! (PS3.18 F.2.2), their uniqueness (F.3.1) and the digits of a DS or IS
//! Number (F.2.3) could not be checked. This reader keeps every member in
//! document order and each number as written, and bounds nesting.

use std::fmt;

/// A JSON value as written: members in document order, numbers as their text.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Json<'a> {
    Null,
    Bool(bool),
    /// The number's text, already checked against the RFC 8259 grammar.
    Number(&'a str),
    String(String),
    Array(Vec<Json<'a>>),
    Object(Vec<(String, Json<'a>)>),
}

impl Json<'_> {
    /// The JSON type's name, for messages.
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "a JSON Boolean",
            Self::Number(_) => "a JSON Number",
            Self::String(_) => "a JSON String",
            Self::Array(_) => "a JSON Array",
            Self::Object(_) => "a JSON Object",
        }
    }
}

/// Why a document is not RFC 8259 JSON, with the line and column (from 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SyntaxError {
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) reason: String,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {} column {}: {}",
            self.line, self.column, self.reason
        )
    }
}

/// Parses `text` as one JSON value, nested at most `max_depth` arrays and
/// objects deep. A leading byte order mark is reported, not skipped: RFC 8259
/// 8.1 forbids adding one to networked JSON text.
pub(super) fn parse(text: &str, max_depth: usize) -> Result<Json<'_>, SyntaxError> {
    let mut parser = Parser {
        text,
        bytes: text.as_bytes(),
        position: 0,
        max_depth,
    };
    if text.starts_with('\u{feff}') {
        return Err(parser.error("the text begins with a byte order mark (RFC 8259 8.1)"));
    }
    parser.whitespace();
    let value = parser.value(0)?;
    parser.whitespace();
    if parser.position < parser.bytes.len() {
        return Err(parser.error("unexpected text after the JSON value"));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    position: usize,
    max_depth: usize,
}

impl<'a> Parser<'a> {
    fn error(&self, reason: impl Into<String>) -> SyntaxError {
        let before = &self.bytes[..self.position.min(self.bytes.len())];
        let line = before.iter().filter(|byte| **byte == b'\n').count() + 1;
        let line_start = before
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        let column = self.text[line_start..self.position.min(self.text.len())]
            .chars()
            .count()
            + 1;
        SyntaxError {
            line,
            column,
            reason: reason.into(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.position += 1;
        }
    }

    fn expect(&mut self, literal: &str) -> Result<(), SyntaxError> {
        if self.bytes[self.position..].starts_with(literal.as_bytes()) {
            self.position += literal.len();
            Ok(())
        } else {
            Err(self.error(format!("expected {literal}")))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json<'a>, SyntaxError> {
        match self.peek() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.expect("true").map(|()| Json::Bool(true)),
            Some(b'f') => self.expect("false").map(|()| Json::Bool(false)),
            Some(b'n') => self.expect("null").map(|()| Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number().map(Json::Number),
            Some(_) => Err(self.error("expected a JSON value")),
            None => Err(self.error("the text ends where a JSON value is expected")),
        }
    }

    fn nest(&self, depth: usize) -> Result<(), SyntaxError> {
        if depth > self.max_depth {
            Err(self.error(format!(
                "arrays and objects are nested deeper than {} levels",
                self.max_depth
            )))
        } else {
            Ok(())
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json<'a>, SyntaxError> {
        self.nest(depth)?;
        self.position += 1;
        let mut members = Vec::new();
        self.whitespace();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.whitespace();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a member name string"));
            }
            let name = self.string()?;
            self.whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error("expected ':' after the member name"));
            }
            self.position += 1;
            self.whitespace();
            let value = self.value(depth)?;
            members.push((name, value));
            self.whitespace();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b'}') => {
                    self.position += 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(self.error("expected ',' or '}' in an object")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json<'a>, SyntaxError> {
        self.nest(depth)?;
        self.position += 1;
        let mut elements = Vec::new();
        self.whitespace();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Json::Array(elements));
        }
        loop {
            self.whitespace();
            elements.push(self.value(depth)?);
            self.whitespace();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(Json::Array(elements));
                }
                _ => return Err(self.error("expected ',' or ']' in an array")),
            }
        }
    }

    /// RFC 8259 6: `-? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?`.
    fn number(&mut self) -> Result<&'a str, SyntaxError> {
        let start = self.position;
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        match self.peek() {
            Some(b'0') => self.position += 1,
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(self.error("a number needs a digit after '-'")),
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("a number needs a digit after '.'"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("a number needs a digit in its exponent"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(self.error("a number shall not have a leading zero"));
        }
        Ok(&self.text[start..self.position])
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.position += 1;
        }
    }

    /// RFC 8259 7, including UTF-16 surrogate pairs in `\u` escapes.
    fn string(&mut self) -> Result<String, SyntaxError> {
        self.position += 1;
        let mut value = String::new();
        loop {
            let run_start = self.position;
            while let Some(byte) = self.peek() {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.position += 1;
            }
            value.push_str(&self.text[run_start..self.position]);
            match self.peek() {
                Some(b'"') => {
                    self.position += 1;
                    return Ok(value);
                }
                Some(b'\\') => value.push(self.escape()?),
                Some(_) => {
                    return Err(self.error(
                        "a control character (U+0000 to U+001F) shall be escaped in a string",
                    ))
                }
                None => return Err(self.error("the text ends inside a string")),
            }
        }
    }

    fn escape(&mut self) -> Result<char, SyntaxError> {
        self.position += 1;
        let Some(byte) = self.peek() else {
            return Err(self.error("the text ends inside an escape"));
        };
        self.position += 1;
        Ok(match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode_escape(),
            _ => {
                self.position -= 1;
                return Err(self.error(format!("'\\{}' is not a JSON escape", char::from(byte))));
            }
        })
    }

    fn unicode_escape(&mut self) -> Result<char, SyntaxError> {
        let first = self.hex4()?;
        if !(0xD800..=0xDFFF).contains(&first) {
            return char::from_u32(first).ok_or_else(|| self.error("invalid \\u escape"));
        }
        if first >= 0xDC00 || !self.bytes[self.position..].starts_with(b"\\u") {
            return Err(self.error("a \\u escape holds an unpaired UTF-16 surrogate"));
        }
        self.position += 2;
        let second = self.hex4()?;
        if !(0xDC00..=0xDFFF).contains(&second) {
            return Err(self.error("a \\u escape holds an unpaired UTF-16 surrogate"));
        }
        let scalar = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
        char::from_u32(scalar).ok_or_else(|| self.error("invalid \\u escape"))
    }

    fn hex4(&mut self) -> Result<u32, SyntaxError> {
        let digits = self
            .bytes
            .get(self.position..self.position + 4)
            .filter(|digits| digits.iter().all(u8::is_ascii_hexdigit))
            .ok_or_else(|| self.error("a \\u escape needs four hexadecimal digits"))?;
        let value = digits.iter().fold(0u32, |value, digit| {
            value * 16 + char::from(*digit).to_digit(16).unwrap_or(0)
        });
        self.position += 4;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, Json};

    #[test]
    fn members_keep_document_order_duplicates_and_number_text() {
        let parsed = parse(r#"{"b": 1.50, "a": [true, null], "b": -0e+2}"#, 8).unwrap();
        assert_eq!(
            parsed,
            Json::Object(vec![
                ("b".into(), Json::Number("1.50")),
                ("a".into(), Json::Array(vec![Json::Bool(true), Json::Null])),
                ("b".into(), Json::Number("-0e+2")),
            ])
        );
    }

    #[test]
    fn escapes_and_surrogate_pairs_decode() {
        assert_eq!(
            parse(r#""a\"\\\/\b\f\n\r\t\u00e9\ud834\udd1e""#, 1).unwrap(),
            Json::String("a\"\\/\u{8}\u{c}\n\r\t\u{e9}\u{1d11e}".into())
        );
    }

    #[test]
    fn each_grammar_violation_is_rejected_with_its_position() {
        for (text, reason) in [
            ("{\"a\":1,}", "expected a member name string"),
            ("[1,]", "expected a JSON value"),
            ("01", "leading zero"),
            ("1.", "digit after '.'"),
            ("1e", "exponent"),
            ("-", "digit after '-'"),
            ("\"a\u{1}\"", "control character"),
            ("\"\\x\"", "not a JSON escape"),
            ("\"\\ud800\"", "unpaired"),
            ("\"\\udc00\\ud800\"", "unpaired"),
            ("[1] 2", "after the JSON value"),
            ("\u{feff}[]", "byte order mark"),
            ("nul", "expected null"),
            ("{\"a\" 1}", "expected ':'"),
            ("\"abc", "ends inside a string"),
        ] {
            let error = parse(text, 4).unwrap_err();
            assert!(
                error.reason.contains(reason),
                "{text:?}: {error} does not mention {reason:?}"
            );
        }
        let error = parse("[\n  1,\n  x]", 4).unwrap_err();
        assert_eq!((error.line, error.column), (3, 3));
    }

    #[test]
    fn nesting_is_bounded() {
        assert!(parse("[[[]]]", 3).is_ok());
        assert!(parse("[[[[]]]]", 3)
            .unwrap_err()
            .reason
            .contains("deeper than 3"));
    }
}
