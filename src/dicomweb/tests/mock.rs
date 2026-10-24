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
    let mut head = format!("HTTP/1.1 {} Mock\r\n", response.status);
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if !response.unframed {
        head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    }
    head.push_str("Connection: close\r\n\r\n");
    let _ = writer.write_all(head.as_bytes());
    let _ = writer.write_all(&response.body);
    let _ = writer.flush();
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
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(MockRequest {
        method,
        target,
        headers,
        body,
    })
}
