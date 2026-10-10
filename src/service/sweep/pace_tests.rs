// Relay pacing for intent-mode batch sales on a virtual clock: a 40-token run stays under a stream
// limit of 30 publishes per rolling minute, and a 429 resends the identical signed body once.
use super::*;
use super::intent_tests::rpc;
use crate::service::intent::Clock;
use std::sync::{Mutex, atomic::{AtomicU64, Ordering::SeqCst}};
#[path = "../../client/test_server.rs"]
mod relay_server;

const RATE_LIMITED: &str = r#"{"error":"stream_rate_limited","retryAfterSec":3}"#;
const ACCEPTED: &str = r#"{"accepted":true}"#;

/// Virtual time: sleeps advance it instantly and are recorded.
#[derive(Default)]
struct TestClock { now: AtomicU64, slept: Mutex<Vec<u64>> }

#[async_trait::async_trait]
impl Clock for TestClock {
    fn now_ms(&self) -> u64 { self.now.load(SeqCst) }
    async fn sleep_ms(&self, ms: u64) { self.slept.lock().unwrap().push(ms); self.now.fetch_add(ms, SeqCst); }
}

/// Intent placement reads its RPC URL from the environment, so the scenario runs in a child
/// process started with the sweep RPC fixture.
fn hermetic<F: std::future::Future<Output = ()>>(name: &str, scenario: impl FnOnce(String) -> F) {
    if let Ok(url) = std::env::var("PACE_RPC") {
        tokio::runtime::Runtime::new().unwrap().block_on(scenario(url)); return;
    }
    let rpc = rpc(5, "open");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &format!("service::sweep::pace_tests::{name}"), "--nocapture"])
        .env("PACE_RPC", &rpc.url).env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
}

/// A live intent-mode sweep of `count` synthetic spend tokens into WETH through `relay`.
async fn sweep(count: u8, relay: &relay_server::Server, clock: Arc<TestClock>, url: &str) -> Output {
    let receive = token::from_registry("WETH", 8453).unwrap();
    let spend: Vec<Address> = (1..=count).map(|i| Address::left_padding_from(&[0x7e, i])).collect();
    let mut policy = tests::policy();
    policy.tokens[0].token = receive.address.clone(); policy.action_mask = "5".into();
    let mut prices = BTreeMap::from([(receive.address.parse().unwrap(), tests::price("1", true))]);
    for token in &spend {
        policy.tokens.push(intent::TokenPolicy { token: token.to_string(), allowed: true, cap: "900000".into(), used: "200000".into(), epoch_start: "90".into() });
        prices.insert(*token, tests::price("1", true));
    }
    let input = serde_json::from_value(serde_json::json!({"chain_id": "8453", "proxy": Address::repeat_byte(6), "receive": "WETH",
        "tokens": spend, "max_usd": "5", "max_loss_bps": 100, "dry_run": false, "wait": 0})).unwrap();
    let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
    let client = Client::new(&relay.url, None);
    run(Context { client: &client, signer, input, provider: evm::read_provider(url).unwrap(), owner: Address::repeat_byte(4),
        receive, prices, policy, max: amount::fixed("5", false).unwrap(), server_cap: None, wait: Wait::MCP,
        pacer: intent::Pacer::new(clock) }).await.unwrap()
}

fn bodies(relay: &relay_server::Server) -> Vec<String> {
    relay.requests.lock().unwrap().iter().map(|r| {
        assert_eq!((r.method.as_str(), r.target.as_str(), r.keyed, r.paid), ("POST", crate::routes::INTENT_ANNOUNCE, false, false));
        r.body.clone()
    }).collect()
}

#[test]
fn forty_token_batch_is_paced_under_the_stream_limit() {
    hermetic("forty_token_batch_is_paced_under_the_stream_limit", |url| async move {
        let clock = Arc::new(TestClock::default());
        let published = Arc::new(Mutex::new(Vec::<u64>::new()));
        let (stream_clock, stream) = (clock.clone(), published.clone());
        // The stream admits 30 publishes per owner per rolling minute; each POST takes one virtual second.
        let relay = relay_server::Server::start_with(move |_, _| {
            let now = stream_clock.now.fetch_add(1000, SeqCst);
            let mut stream = stream.lock().unwrap();
            if stream.iter().filter(|t| **t + 60_000 > now).count() >= 30 {
                return (429, "Retry-After: 30\r\n".into(), RATE_LIMITED.into());
            }
            stream.push(now);
            (200, String::new(), ACCEPTED.into())
        });
        let output = sweep(40, &relay, clock.clone(), &url).await;
        let rows = &output.tokens[1..];
        assert_eq!(rows.iter().filter(|r| r.outcome == "placed" && r.announce_status.as_deref() == Some("accepted")).count(), 40, "{rows:?}");
        assert_eq!(bodies(&relay).len(), 40, "no 429 and no resend");
        let published = published.lock().unwrap();
        for t in published.iter() {
            assert!(published.iter().filter(|s| **s <= *t && **s + 60_000 > *t).count() <= 25, "window ending {t} over 25: {published:?}");
        }
        assert_eq!(*clock.slept.lock().unwrap(), vec![35_000], "one virtual wait for the oldest publish to leave the window");
    });
}

#[test]
fn twice_rate_limited_token_fails_and_the_next_is_placed() {
    hermetic("twice_rate_limited_token_fails_and_the_next_is_placed", |url| async move {
        let limited = (429, "Retry-After: 500\r\n".to_string(), RATE_LIMITED.to_string());
        let relay = relay_server::Server::start(vec![limited.clone(), limited, (200, String::new(), ACCEPTED.into())]);
        let clock = Arc::new(TestClock::default());
        let output = sweep(2, &relay, clock.clone(), &url).await;
        let (first, second) = (&output.tokens[1], &output.tokens[2]);
        assert_eq!((first.outcome.as_str(), first.reason.as_deref(), first.intent_id.as_deref()), ("failed", Some("relay_rate_limited"), None));
        assert!(first.error.as_deref().unwrap().starts_with("HTTP 429"), "{first:?}");
        assert_eq!((second.outcome.as_str(), second.announce_status.as_deref()), ("placed", Some("accepted")));
        let bodies = bodies(&relay);
        assert_eq!(bodies.len(), 3, "one resend, then the next token");
        assert_eq!(bodies[0], bodies[1]);
        assert_ne!(bodies[2], bodies[0]);
        assert_eq!(*clock.slept.lock().unwrap(), vec![120_000], "Retry-After wins over retryAfterSec and is capped");
    });
}

#[test]
fn retry_resends_the_byte_identical_signed_body() {
    hermetic("retry_resends_the_byte_identical_signed_body", |url| async move {
        let relay = relay_server::Server::start(vec![(429, String::new(), RATE_LIMITED.into()), (200, String::new(), ACCEPTED.into())]);
        let clock = Arc::new(TestClock::default());
        let output = sweep(1, &relay, clock.clone(), &url).await;
        let row = &output.tokens[1];
        assert_eq!((row.outcome.as_str(), row.announce_status.as_deref()), ("placed", Some("accepted")), "{row:?}");
        let bodies = bodies(&relay);
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0].as_bytes(), bodies[1].as_bytes(), "same order, nonce, signature and order hash");
        let sent: serde_json::Value = serde_json::from_str(&bodies[0]).unwrap();
        let order = intent_sale::order(&serde_json::from_value(sent["announce"]["order"].clone()).unwrap()).unwrap();
        assert_eq!(row.intent_id.as_deref(), Some(format!("{:?}", order_types::order_id(&order)).as_str()));
        assert_eq!(*clock.slept.lock().unwrap(), vec![3_000], "retryAfterSec when no Retry-After header");
    });
}

#[test]
fn non_429_relay_error_is_not_retried() {
    hermetic("non_429_relay_error_is_not_retried", |url| async move {
        let relay = relay_server::Server::start(vec![(500, String::new(), r#"{"error":"internal"}"#.into()), (200, String::new(), ACCEPTED.into())]);
        let clock = Arc::new(TestClock::default());
        let output = sweep(2, &relay, clock.clone(), &url).await;
        let (first, second) = (&output.tokens[1], &output.tokens[2]);
        assert_eq!((first.outcome.as_str(), first.reason.as_deref()), ("failed", None));
        assert!(first.error.as_deref().unwrap().starts_with("HTTP 500"), "{first:?}");
        assert_eq!(second.outcome, "placed");
        assert_eq!(bodies(&relay).len(), 2, "no resend after a non-429 error");
        assert!(clock.slept.lock().unwrap().is_empty());
    });
}
