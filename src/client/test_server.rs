// Loopback HTTP fixture for transport security regressions; run only on remote test hosts.
// Exports: Server and Request observations, without retaining credential values.
// Deps: std TCP/thread primitives and serde_json.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};

pub(super) struct Request {
    pub method: String,
    pub target: String,
    pub keyed: bool,
    pub paid: bool,
    pub body: String,
}

pub(super) struct Server {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    pub fn start(responses: Vec<(u16, String, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (observed, stopped) = (requests.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) { break; }
                let mut stream = stream.unwrap();
                let request = read_request(&mut stream);
                let mut recorded = observed.lock().unwrap();
                let index = recorded.len().min(responses.len() - 1);
                recorded.push(request);
                let (status, headers, body) = &responses[index];
                write!(stream, "HTTP/1.1 {status} Fixture\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self { url, requests, stop, worker: Some(worker) }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        self.worker.take().unwrap().join().unwrap();
    }
}

fn read_request(stream: &mut TcpStream) -> Request {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let method = line.split_whitespace().next().unwrap().to_string();
    let target = line.split_whitespace().nth(1).unwrap().to_string();
    let (mut keyed, mut paid, mut length) = (false, false, 0);
    loop {
        line.clear();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if line == "\r\n" { break; }
        if let Some((name, value)) = line.split_once(':') {
            keyed |= name.eq_ignore_ascii_case("x-api-key") && !value.trim().is_empty();
            paid |= name.eq_ignore_ascii_case("x-payment") && !value.trim().is_empty();
            if name.eq_ignore_ascii_case("content-length") { length = value.trim().parse().unwrap(); }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    Request { method, target, keyed, paid, body: String::from_utf8(body).unwrap() }
}
