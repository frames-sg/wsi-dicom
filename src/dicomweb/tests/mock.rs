//! An in-process HTTP/1.1 origin server on the loopback interface, built on
//! std's TcpListener: one request per connection, answered by a handler.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// A request as the mock server read it.
#[derive(Debug, Clone)]
pub(crate) struct MockRequest {
    pub(crate) method: String,
    /// Path and query.
    pub(crate) target: String,
    /// Header fields, names lowercase.
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl MockRequest {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
    }

    /// The path without the query.
    pub(crate) fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or_default()
    }

    /// The percent-decoded value of query parameter `name`.
    pub(crate) fn decoded(&self, name: &str) -> Option<String> {
        self.query(name).map(percent_decode)
    }

    /// The value of query parameter `name`.
    pub(crate) fn query(&self, name: &str) -> Option<&str> {
        self.target
            .split_once('?')?
            .1
            .split('&')
            .find_map(|pair| match pair.split_once('=') {
                Some((key, value)) if key == name => Some(value),
                None if pair == name => Some(""),
                _ => None,
            })
    }
}

/// A response to write.
#[derive(Debug, Clone, Default)]
pub(crate) struct MockResponse {
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
    /// Leave out Content-Length; the body ends when the connection closes.
    pub(crate) unframed: bool,
    /// Wait this long before answering.
    pub(crate) delay: Option<Duration>,
    /// Close the connection without writing anything.
    pub(crate) silent: bool,
    /// The status line to write instead of `HTTP/1.1 {status} Mock`.
    pub(crate) status_line: Option<String>,
    /// Send the body with Transfer-Encoding: chunked.
    pub(crate) chunked: bool,
    /// Write Content-Length even when the status has no payload (204, 304).
    pub(crate) length_without_payload: bool,
    /// Keep an error response without the Status Report the origin server
    /// adds to it.
    pub(crate) bare: bool,
}

impl MockResponse {
    pub(crate) fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), content_type.into())],
            body: body.into(),
            ..Self::default()
        }
    }

    pub(crate) fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::default()
        }
    }

    pub(crate) fn with(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

type Handler = dyn Fn(&MockRequest) -> MockResponse + Send + Sync;

/// A running mock server; stopped when dropped.
pub(crate) struct MockServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<MockRequest>>>,
    thread: Option<JoinHandle<()>>,
}

impl MockServer {
    pub(crate) fn start(
        handler: impl Fn(&MockRequest) -> MockResponse + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Handler> = Arc::new(handler);
        let thread = {
            let stop = Arc::clone(&stop);
            let requests = Arc::clone(&requests);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else {
                        continue;
                    };
                    let handler = Arc::clone(&handler);
                    let requests = Arc::clone(&requests);
                    std::thread::spawn(move || serve(stream, &*handler, &requests));
                }
            })
        };
        Self {
            address,
            stop,
            requests,
            thread: Some(thread),
        }
    }

    /// `http://127.0.0.1:port` followed by `path`.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    /// The requests received so far.
    pub(crate) fn requests(&self) -> Vec<MockRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(stream: TcpStream, handler: &Handler, requests: &Mutex<Vec<MockRequest>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let Some(request) = read_request(&mut reader) else {
        return;
    };
    requests.lock().unwrap().push(request.clone());
    let response = handler(&request);
    if let Some(delay) = response.delay {
        std::thread::sleep(delay);
    }
    if response.silent {
        return;
    }
    let mut head = response
        .status_line
        .clone()
        .unwrap_or_else(|| format!("HTTP/1.1 {} Mock", response.status));
    head.push_str("\r\n");
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    let no_payload = matches!(response.status, 204 | 304);
    if !response.unframed && (!no_payload || response.length_without_payload) {
        head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    }
    if response.chunked {
        head.push_str("Transfer-Encoding: chunked\r\n");
    }
    head.push_str("Connection: close\r\n\r\n");
    let _ = writer.write_all(head.as_bytes());
    if response.chunked {
        let _ = writer.write_all(format!("{:X}\r\n", response.body.len()).as_bytes());
        let _ = writer.write_all(&response.body);
        let _ = writer.write_all(b"\r\n0\r\n\r\n");
    } else {
        let _ = writer.write_all(&response.body);
    }
    let _ = writer.flush();
}

/// RFC 3986 percent-decoding; an invalid escape is kept as written.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| text.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// A chunked request body (RFC 7230 4.1), or `None` if it is malformed.
fn read_chunked(reader: &mut BufReader<TcpStream>) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let size = usize::from_str_radix(line.trim().split(';').next()?, 16).ok()?;
        if size == 0 {
            let mut trailer = String::new();
            reader.read_line(&mut trailer).ok()?;
            return Some(body);
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..]).ok()?;
        let mut crlf = [0; 2];
        reader.read_exact(&mut crlf).ok()?;
    }
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Option<MockRequest> {
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut words = line.split_whitespace();
    let method = words.next()?.to_string();
    let target = words.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':')?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let chunked = headers
        .iter()
        .any(|(name, value)| name == "transfer-encoding" && value.contains("chunked"));
    let body = if chunked {
        read_chunked(reader)?
    } else {
        let length = headers
            .iter()
            .find(|(name, _)| name == "content-length")
            .and_then(|(_, value)| value.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).ok()?;
        body
    };
    Some(MockRequest {
        method,
        target,
        headers,
        body,
    })
}
