// Regression tests for requested/forced MCP dry-runs and unchanged live signing.
// Exports: module-local tests; subprocesses isolate RPC environment overrides.
// Deps: MCP handlers, fixture HTTP server, real local signer with call counting.

use super::*;
use super::dry_run_fixture::Fixture;
use alloy::primitives::{Address, B256, Signature};
use alloy::sol_types::SolCall;
use async_trait::async_trait;
use crate::order_types::{IntentSettlerV3, UserProxyV6};
use crate::signer::local::LocalKey;
use std::sync::atomic::{AtomicUsize, Ordering};

struct RecordingSigner {
    key: LocalKey,
    calls: AtomicUsize,
}

#[async_trait]
impl Signer for RecordingSigner {
    fn address(&self) -> Address { self.key.address() }

    async fn sign_message(&self, message: &[u8]) -> Result<Signature> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.key.sign_message(message).await
    }

    async fn sign_hash(&self, digest: B256) -> Result<Signature> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.key.sign_hash(digest).await
    }
}

fn run_case(name: &str, is_intent: bool, requested: bool, allowed: bool) {
    if std::env::var("AGENTSWAP_DRY_RUN_CASE").as_deref() == Ok(name) {
        tokio::runtime::Runtime::new().unwrap().block_on(check_output(is_intent, requested, allowed));
        return;
    }
    let fixture = Fixture::start();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &format!("mcp::dry_run_tests::{name}"), "--nocapture"])
        .env("AGENTSWAP_DRY_RUN_CASE", name)
        .env("AGENTSWAP_RPC_URL_8453", &fixture.url)
        .output().expect("isolated test subprocess");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let calls = fixture.calls.lock().unwrap();
    assert!(calls.contains(&hex::encode(UserProxyV6::policyOfCall::SELECTOR)));
    let selector = if is_intent { UserProxyV6::hashIntentAuthorizationCall::SELECTOR } else { UserProxyV6::hashAgentOrderCall::SELECTOR };
    assert!(calls.contains(&hex::encode(selector)), "digest parity RPC missing");
    if is_intent { assert!(calls.contains(&hex::encode(IntentSettlerV3::orderHashCall::SELECTOR))); }
    if requested || !allowed {
        assert!(!calls.contains(&crate::routes::INTENT_ANNOUNCE.to_string()));
        assert!(!calls.contains(&hex::encode(UserProxyV6::isIntentAuthorizedCall::SELECTOR)), "dry-run must not construct a signed authorization for RPC");
    }
}

async fn check_output(is_intent: bool, requested: bool, allowed: bool) {
    let signer = Arc::new(RecordingSigner {
        key: LocalKey::from_private_key(&"01".repeat(32)).unwrap(), calls: AtomicUsize::new(0),
    });
    let client = Client::new(&std::env::var("AGENTSWAP_RPC_URL_8453").unwrap(), None);
    let server = AgentSwapMcp::new(Config {
        client: client.clone(), intent_client: client, signer: Some(signer.clone()),
        allow_trade: allowed, trade_max_amount: None,
    });
    let value = if is_intent {
        let Json(value) = server.intent_place(Parameters(intent_input(requested))).await.expect("intent preview");
        serde_json::to_value(value).unwrap()
    } else {
        let Json(value) = server.trade(Parameters(trade_input(requested, allowed))).await.expect("trade preview");
        serde_json::to_value(value).unwrap()
    };
    let dry_run = requested || !allowed;
    assert_eq!(value["dry_run"], dry_run);
    assert!(value["order"].is_object());
    assert_eq!(value["digest"].as_str().unwrap().len(), 66);
    if is_intent { assert!(value["authorization"].is_object()); }
    else { assert_eq!(value["quote"]["output"], "1000"); }
    if dry_run {
        let leaked: Vec<_> = ["signature", "envelope", "self_submit"].into_iter().filter(|field| value.get(field).is_some()).collect();
        let signatures = signer.calls.load(Ordering::SeqCst);
        assert!(leaked.is_empty() && signatures == 0, "dry-run released authority: fields={leaked:?}, signing calls={signatures}; expected absent signature/envelope/signed calldata and zero signing calls");
    } else {
        assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
        let signature: Signature = value["signature"].as_str().unwrap().parse().unwrap();
        let digest: B256 = value["digest"].as_str().unwrap().parse().unwrap();
        assert_eq!(signature.recover_address_from_prehash(&digest).unwrap(), signer.address());
        if is_intent { assert!(value["envelope"].as_str().unwrap().len() > 2); assert_eq!(value["relay"]["accepted"], true); }
        else { assert_eq!(value["self_submit"]["function"], "executeAsAgent"); assert!(value["self_submit"]["calldata"].as_str().unwrap().len() > 2); }
    }
}

fn trade_input(dry_run: bool, allowed: bool) -> trade::TradeInput {
    trade::TradeInput {
        chain_id: "base".to_string(), from: "USDC".to_string(), to: "WETH".to_string(),
        amount: "1".to_string(), slippage: None,
        min_out: if !dry_run && allowed { Some("1".to_string()) } else { None },
        max_amount: None, mode: "agent-order".to_string(), proxy: format!("{:?}", Address::repeat_byte(2)),
        nonce: Some("1".to_string()), deadline_secs: Some(120), dry_run, self_submit: dry_run || !allowed,
    }
}

fn intent_input(dry_run: bool) -> intent::PlaceInput {
    intent::PlaceInput {
        chain_id: "base".to_string(), proxy_owner: format!("{:?}", Address::repeat_byte(4)),
        from: "USDC".to_string(), to: "WETH".to_string(), amount: "1".to_string(),
        start_out: "2".to_string(), end_out: "1".to_string(), decay_secs: Some(1),
        duration_secs: Some(1), deadline_secs: None, relay: true, self_submit: false, dry_run, max_amount: None,
    }
}

#[test]
fn requested_trade_dry_run_never_releases_authority() {
    run_case("requested_trade_dry_run_never_releases_authority", false, true, true);
}

#[test]
fn forced_trade_dry_run_never_releases_authority() {
    run_case("forced_trade_dry_run_never_releases_authority", false, false, false);
}

#[test]
fn requested_intent_dry_run_never_releases_authority() {
    run_case("requested_intent_dry_run_never_releases_authority", true, true, true);
}

#[test]
fn forced_intent_dry_run_never_releases_authority() {
    run_case("forced_intent_dry_run_never_releases_authority", true, false, false);
}

#[test]
fn live_trade_retains_signature_and_signed_calldata() {
    run_case("live_trade_retains_signature_and_signed_calldata", false, false, true);
}

#[test]
fn live_intent_retains_signature_envelope_and_relay() {
    run_case("live_intent_retains_signature_envelope_and_relay", true, false, true);
}

#[tokio::test]
async fn recording_signer_counts_message_and_hash_signatures() {
    let signer = RecordingSigner {
        key: LocalKey::from_private_key(&"01".repeat(32)).unwrap(), calls: AtomicUsize::new(0),
    };
    signer.sign_message(b"fixture counter").await.unwrap();
    assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
    signer.sign_hash(B256::ZERO).await.unwrap();
    assert_eq!(signer.calls.load(Ordering::SeqCst), 2);
}

fn run_rejection(name: &str, selector: [u8; 4], is_intent: bool, expected: &str) {
    if std::env::var("AGENTSWAP_DRY_RUN_CASE").as_deref() == Ok(name) {
        tokio::runtime::Runtime::new().unwrap().block_on(check_rejection(is_intent, expected));
        return;
    }
    let fixture = Fixture::with_bad_digest(Some(selector));
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &format!("mcp::dry_run_tests::{name}"), "--nocapture"])
        .env("AGENTSWAP_DRY_RUN_CASE", name)
        .env("AGENTSWAP_RPC_URL_8453", &fixture.url)
        .output().expect("isolated rejection subprocess");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let calls = fixture.calls.lock().unwrap();
    assert!(calls.contains(&hex::encode(selector)));
    assert!(!calls.contains(&crate::routes::INTENT_ANNOUNCE.to_string()));
}

async fn check_rejection(is_intent: bool, expected: &str) {
    let signer = Arc::new(RecordingSigner {
        key: LocalKey::from_private_key(&"01".repeat(32)).unwrap(), calls: AtomicUsize::new(0),
    });
    let client = Client::new(&std::env::var("AGENTSWAP_RPC_URL_8453").unwrap(), None);
    let server = AgentSwapMcp::new(Config {
        client: client.clone(), intent_client: client, signer: Some(signer.clone()),
        allow_trade: true, trade_max_amount: None,
    });
    let error = if is_intent {
        server.intent_place(Parameters(intent_input(true))).await.err().expect("reject intent parity mismatch")
    } else {
        server.trade(Parameters(trade_input(true, true))).await.err().expect("reject trade parity mismatch")
    };
    assert!(error.contains(expected), "{error}");
    assert_eq!(signer.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn dry_run_rejects_agent_order_digest_mismatch() {
    run_rejection("dry_run_rejects_agent_order_digest_mismatch", UserProxyV6::hashAgentOrderCall::SELECTOR, false, "local AgentOrder digest does not match");
}

#[test]
fn dry_run_rejects_intent_order_hash_mismatch() {
    run_rejection("dry_run_rejects_intent_order_hash_mismatch", IntentSettlerV3::orderHashCall::SELECTOR, true, "local intent id does not match");
}

#[test]
fn dry_run_rejects_authorization_digest_mismatch() {
    run_rejection("dry_run_rejects_authorization_digest_mismatch", UserProxyV6::hashIntentAuthorizationCall::SELECTOR, true, "local authorization digest does not match");
}
