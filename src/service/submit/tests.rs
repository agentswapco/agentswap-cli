// Tests for sign-first submission against a scripted JSON-RPC server: refused, unanswered,
// reverted, confirmed and never-mined transactions, and the redaction of the RPC URL.
// Exports: no production symbols.
// Deps: parent submit module, crate::service::test_rpc, crate::signer::local, serde_json.

use super::*;
use crate::service::test_rpc::{failure, ok, TestRpc};
use crate::signer::local::LocalKey;
use alloy::primitives::keccak256;
use serde_json::{json, Value};
use std::sync::Mutex;

#[derive(Clone, Copy)]
enum Broadcast { Accept, Refuse, Drop }

#[derive(Clone, Copy)]
enum Receipt { Pending, Status(u8), Fail }

/// Starts a chain whose broadcast and receipt polls follow the script; the last receipt step
/// repeats. Returns the server and the hash of the raw transaction it received.
fn chain(broadcast: Broadcast, receipts: Vec<Receipt>) -> (TestRpc, Arc<Mutex<Option<B256>>>) {
    let sent = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&sent);
    let mut polls = 0;
    let rpc = TestRpc::start(move |body| match body["method"].as_str().unwrap_or_default() {
        "eth_sendRawTransaction" => {
            let raw = hex::decode(body["params"][0].as_str().unwrap().trim_start_matches("0x")).unwrap();
            *seen.lock().unwrap() = Some(keccak256(&raw));
            match broadcast {
                Broadcast::Accept => Some(ok(body, json!(keccak256(&raw)))),
                Broadcast::Refuse => Some(failure(body, "insufficient funds for gas * price + value")),
                Broadcast::Drop => None,
            }
        }
        "eth_getTransactionReceipt" => {
            let step = receipts[polls.min(receipts.len() - 1)];
            polls += 1;
            Some(receipt_reply(body, step))
        }
        method => Some(fill_reply(body, method)),
    });
    (rpc, sent)
}

fn fill_reply(body: &Value, method: &str) -> Value {
    match method {
        "eth_chainId" => ok(body, json!("0x2105")),
        "eth_getTransactionCount" => ok(body, json!("0x0")),
        "eth_estimateGas" => ok(body, json!("0x30000")),
        "eth_feeHistory" => ok(body, json!({
            "oldestBlock": "0x1", "baseFeePerGas": ["0x3b9aca00", "0x3b9aca00"],
            "gasUsedRatio": [0.5], "reward": [["0x3b9aca00"]],
        })),
        _ => failure(body, "method not found"),
    }
}

fn receipt_reply(body: &Value, step: Receipt) -> Value {
    let status = match step {
        Receipt::Pending => return ok(body, Value::Null),
        Receipt::Fail => return failure(body, "rate limited"),
        Receipt::Status(status) => status,
    };
    ok(body, json!({
        "type": "0x2", "status": format!("0x{status:x}"), "cumulativeGasUsed": "0x5208", "logs": [],
        "logsBloom": format!("0x{}", "0".repeat(512)), "transactionHash": body["params"][0],
        "transactionIndex": "0x0", "blockHash": format!("0x{}", "11".repeat(32)), "blockNumber": "0x1",
        "gasUsed": "0x5208", "effectiveGasPrice": "0x3b9aca00", "from": format!("{:?}", Address::repeat_byte(1)),
        "to": format!("{:?}", Address::repeat_byte(2)), "contractAddress": null,
    }))
}

const FAST: Wait = Wait {
    receipt: Duration::from_millis(600),
    poll: Duration::from_millis(50),
    call: Duration::from_secs(2),
};

async fn submit(rpc: &TestRpc) -> Result<Submission> {
    let signer: Arc<dyn Signer> = Arc::new(LocalKey::from_private_key(&"01".repeat(32)).unwrap());
    send(&rpc.url, signer, Address::repeat_byte(2), Bytes::from_static(&[0xab]), FAST).await
}

fn sent_hash(sent: &Mutex<Option<B256>>) -> B256 {
    sent.lock().unwrap().expect("a raw transaction reached the RPC")
}

#[tokio::test]
async fn a_mined_receipt_with_status_one_is_confirmed() {
    let (rpc, sent) = chain(Broadcast::Accept, vec![Receipt::Pending, Receipt::Status(1)]);
    let submission = submit(&rpc).await.expect("sent");
    assert_eq!(submission, Submission { hash: sent_hash(&sent), status: TxStatus::Confirmed, error: None });
}

#[tokio::test]
async fn a_mined_receipt_with_status_zero_is_reverted_and_keeps_the_hash() {
    let (rpc, sent) = chain(Broadcast::Accept, vec![Receipt::Status(0)]);
    let submission = submit(&rpc).await.expect("sent");
    assert_eq!(submission, Submission { hash: sent_hash(&sent), status: TxStatus::Reverted, error: None });
}

#[tokio::test]
async fn a_refused_broadcast_is_an_error_and_no_receipt_is_awaited() {
    let (rpc, _) = chain(Broadcast::Refuse, vec![Receipt::Status(1)]);
    let error = submit(&rpc).await.expect_err("the RPC refused the transaction");
    assert!(format!("{error}").contains("not broadcast"), "{error}");
    assert_eq!(rpc.called("eth_getTransactionReceipt"), 0);
}

#[tokio::test]
async fn an_unanswered_broadcast_is_unknown_with_the_hash_and_a_redacted_url() {
    let (rpc, sent) = chain(Broadcast::Drop, vec![Receipt::Status(1)]);
    let submission = submit(&rpc).await.expect("the transaction may have been sent");
    assert_eq!((submission.hash, submission.status), (sent_hash(&sent), TxStatus::Unknown));
    let error = submission.error.expect("why the outcome is unknown");
    assert!(error.contains("eth_sendRawTransaction failed"), "{error}");
    assert!(!error.contains("KEYPATH") && !error.contains("SECRET"), "{error}");
}

#[tokio::test]
async fn a_failed_receipt_poll_is_retried_until_the_receipt_arrives() {
    let (rpc, _) = chain(Broadcast::Accept, vec![Receipt::Fail, Receipt::Fail, Receipt::Status(1)]);
    assert_eq!(submit(&rpc).await.expect("sent").status, TxStatus::Confirmed);
    assert_eq!(rpc.called("eth_getTransactionReceipt"), 3);
}

#[tokio::test]
async fn a_receipt_wait_that_ends_without_a_receipt_is_unknown() {
    for (step, expected) in [(Receipt::Pending, "no receipt within"), (Receipt::Fail, "last error")] {
        let (rpc, sent) = chain(Broadcast::Accept, vec![step]);
        let submission = submit(&rpc).await.expect("sent");
        assert_eq!((submission.hash, submission.status), (sent_hash(&sent), TxStatus::Unknown));
        let error = submission.error.expect("why the outcome is unknown");
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("SECRET"), "{error}");
    }
}

#[test]
fn not_confirmed_maps_reverted_and_unknown_to_their_exit_statuses() {
    assert_eq!(NotConfirmed::check(None, None, None), None);
    assert_eq!(NotConfirmed::check(Some("0xab"), Some(TxStatus::Confirmed), None), None);
    let reverted = NotConfirmed::check(Some("0xab"), Some(TxStatus::Reverted), None).unwrap();
    assert_eq!(reverted.exit_code(), EXIT_REVERTED);
    assert_eq!(reverted.to_string(), "transaction 0xab was mined and reverted");
    let unknown = NotConfirmed::check(Some("0xcd"), Some(TxStatus::Unknown), Some("no receipt within 120 s")).unwrap();
    assert_eq!(unknown.exit_code(), EXIT_UNKNOWN);
    assert!(unknown.to_string().starts_with("transaction 0xcd was sent but its outcome is unknown (no receipt within 120 s)"));
    assert_ne!(EXIT_REVERTED, EXIT_UNKNOWN);
    assert!(![0, 1, 2].contains(&EXIT_REVERTED) && ![0, 1, 2].contains(&EXIT_UNKNOWN));
}

#[test]
fn tx_status_words_match_their_json_form() {
    for status in [TxStatus::Confirmed, TxStatus::Reverted, TxStatus::Unknown] {
        assert_eq!(serde_json::to_value(status).unwrap(), status.as_str());
    }
}

#[test]
fn the_exit_status_help_names_every_status_the_code_returns() {
    for status in [0, 1, 2, EXIT_REVERTED, EXIT_UNKNOWN] {
        assert!(EXIT_STATUS_HELP.contains(&format!(" {status} ")), "{status}");
    }
    use clap::CommandFactory;
    let help = crate::cli::Cli::command().render_long_help().to_string();
    assert!(help.contains(EXIT_STATUS_HELP), "{help}");
}
