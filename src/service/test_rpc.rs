// Scripted JSON-RPC server for service tests: each request body goes to a handler that returns
// the reply body, or None to close the connection unanswered.
// Exports: TestRpc, ok, failure.
// Deps: std TCP/thread primitives, serde_json.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct TestRpc {
    /// Listener URL with a keyed path and query, so a test can check neither reaches output.
    pub url: String,
    methods: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl TestRpc {
    pub fn start(mut handler: impl FnMut(&Value) -> Option<Value> + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/KEYPATH?key=SECRET", listener.local_addr().unwrap());
        let methods = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen, stopped) = (Arc::clone(&methods), Arc::clone(&stop));
        let worker = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) { break; }
                let mut stream = stream.unwrap();
                let body = read_body(&mut stream);
                seen.lock().unwrap().push(body["method"].as_str().unwrap_or_default().to_string());
                if let Some(reply) = handler(&body) {
                    let text = reply.to_string();
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
                }
            }
        });
        Self { url, methods, stop, worker: Some(worker) }
    }

    pub fn called(&self, method: &str) -> usize {
        self.methods.lock().unwrap().iter().filter(|seen| *seen == method).count()
    }
}

impl Drop for TestRpc {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let address = self.url.trim_start_matches("http://").split('/').next().unwrap().to_string();
        let _ = TcpStream::connect(address);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn read_body(stream: &mut TcpStream) -> Value {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut reader = BufReader::new(stream);
    let (mut line, mut length) = (String::new(), 0);
    reader.read_line(&mut line).unwrap();
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" || line.is_empty() { break; }
        if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap_or(Value::Null)
}

pub fn ok(body: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": body["id"], "result": result})
}

pub fn failure(body: &Value, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": body["id"], "error": {"code": -32000, "message": message}})
}
