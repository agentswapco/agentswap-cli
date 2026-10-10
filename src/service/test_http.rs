// Loopback HTTP fixture for service tests: a handler maps each request target to a status and body.
// Exports: TestHttp, which records every request target it answered.
// Deps: std TCP/thread primitives.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct TestHttp {
    pub url: String,
    targets: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl TestHttp {
    pub fn start(handler: impl Fn(&str) -> (u16, String) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let targets = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen, stopped) = (Arc::clone(&targets), Arc::clone(&stop));
        let worker = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) { break; }
                let mut stream = stream.unwrap();
                let target = read_target(&mut stream);
                seen.lock().unwrap().push(target.clone());
                let (status, body) = handler(&target);
                let _ = write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        Self { url, targets, stop, worker: Some(worker) }
    }

    pub fn targets(&self) -> Vec<String> {
        self.targets.lock().unwrap().clone()
    }
}

impl Drop for TestHttp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        self.worker.take().unwrap().join().unwrap();
    }
}

fn read_target(stream: &mut TcpStream) -> String {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut reader = BufReader::new(stream);
    let (mut line, mut length) = (String::new(), 0);
    reader.read_line(&mut line).unwrap();
    let target = line.split_whitespace().nth(1).unwrap_or_default().to_string();
    let mut header = String::new();
    loop {
        header.clear();
        reader.read_line(&mut header).unwrap();
        if header == "\r\n" || header.is_empty() { break; }
        if let Some((key, value)) = header.split_once(':')
            && key.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    target
}
