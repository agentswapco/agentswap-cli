// HTTP fixtures for successful quote, relay and V6 RPC parity responses.
// Exports: Fixture with endpoint and observed request paths/selectors for MCP/CLI tests.
// Deps: std TCP/thread primitives, serde_json, existing protocol ABI and hashes.

use crate::order_types::{self, IntentSettlerV3, UserProxyFactoryV6, UserProxyV6};
use alloy::primitives::{Address, U256};
use alloy::sol_types::{SolCall, SolValue};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

pub(super) struct Fixture {
    pub url: String,
    pub calls: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Fixture {
    pub fn start() -> Self {
        Self::with_bad_digest(None)
    }

    pub fn with_bad_digest(bad_digest: Option<[u8; 4]>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        let url = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (recorded, stopped) = (calls.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) { break; }
                serve(stream.expect("fixture connection"), &recorded, bad_digest);
            }
        });
        Self { url, calls, stop, worker: Some(worker) }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        self.worker.take().unwrap().join().expect("fixture worker");
    }
}

fn serve(mut stream: TcpStream, calls: &Mutex<Vec<String>>, bad_digest: Option<[u8; 4]>) {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut reader = BufReader::new(&mut stream);
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let path = first.split_whitespace().nth(1).expect("HTTP path");
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" { break; }
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case("content-length") { length = value.trim().parse().unwrap(); }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let result = response(path, &body, calls, bad_digest).to_string();
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{result}", result.len()).unwrap();
}

fn response(path: &str, body: &Value, calls: &Mutex<Vec<String>>, bad_digest: Option<[u8; 4]>) -> Value {
    calls.lock().unwrap().push(path.to_string());
    if path == crate::routes::QUOTE {
        return json!({"router": format!("{:?}", Address::repeat_byte(3)), "output": "1000", "calldata": "0x1234"});
    }
    if path == crate::routes::INTENT_ANNOUNCE { return json!({"accepted": true}); }
    assert_eq!(body["method"], "eth_call", "unexpected RPC request: {body}");
    let transaction = &body["params"][0];
    let data = transaction.get("input").or_else(|| transaction.get("data")).unwrap().as_str().unwrap();
    let bytes = hex::decode(data.trim_start_matches("0x")).unwrap();
    calls.lock().unwrap().push(hex::encode(&bytes[..4]));
    let address: Address = transaction["to"].as_str().unwrap().parse().unwrap();
    let result = if bad_digest.as_ref().is_some_and(|selector| bytes.starts_with(selector)) {
        vec![0; 32]
    } else { rpc_result(&bytes, address) };
    json!({"jsonrpc": "2.0", "id": body["id"], "result": format!("0x{}", hex::encode(result))})
}

fn rpc_result(data: &[u8], proxy: Address) -> Vec<u8> {
    let selector = &data[..4];
    if selector == UserProxyV6::policyOfCall::SELECTOR {
        return (U256::from(u64::MAX), U256::from(60), U256::from(3), U256::from(7)).abi_encode();
    }
    if selector == UserProxyFactoryV6::proxyOfCall::SELECTOR { return Address::repeat_byte(2).abi_encode(); }
    if selector == UserProxyV6::isAgentNonceUsedCall::SELECTOR { return false.abi_encode(); }
    if selector == UserProxyV6::isIntentAuthorizedCall::SELECTOR { return true.abi_encode(); }
    if selector == UserProxyV6::hashAgentOrderCall::SELECTOR {
        let call = UserProxyV6::hashAgentOrderCall::abi_decode(data).unwrap();
        return order_types::signing_hash(&call.o, &order_types::proxy_domain(8453, proxy)).abi_encode();
    }
    if selector == IntentSettlerV3::orderHashCall::SELECTOR {
        let call = IntentSettlerV3::orderHashCall::abi_decode(data).unwrap();
        return order_types::order_id(&call.o).abi_encode();
    }
    if selector == UserProxyV6::hashIntentAuthorizationCall::SELECTOR {
        let call = UserProxyV6::hashIntentAuthorizationCall::abi_decode(data).unwrap();
        return order_types::signing_hash(&call.auth, &order_types::proxy_domain(8453, proxy)).abi_encode();
    }
    panic!("unexpected RPC selector: {}", hex::encode(selector));
}
