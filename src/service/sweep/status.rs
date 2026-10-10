// Bounded intent status wait for batch sales: one intent-index history read per round, then one
// batched IntentLensV3 read for the intents the index cannot settle or when it is unreachable.
// Works for intents published without an on-chain announce; no log scans.
use super::{Input, Output, Row};
use crate::{evm, service::{intent, intentscan::{self, Origins}}};
use alloy::{primitives::{Address, B256}, providers::DynProvider};
use std::{collections::BTreeMap, time::{Duration, Instant}};

/// Pause between status rounds.
const POLL: Duration = Duration::from_secs(5);

fn terminal(status: &str) -> bool { matches!(status, "filled" | "expired" | "cancelled" | "dead" | "not_placed") }

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Polls accepted intents until each is terminal or `input.wait` seconds pass. `since_ms` bounds
/// the history read to intents created during this run.
pub(super) async fn wait(input: &Input, owner: Address, provider: &DynProvider, origins: &Origins<'_>, since_ms: u64, output: &mut Output) {
    let duration = Duration::from_secs(input.wait.unwrap_or(0));
    if duration.is_zero() { return; }
    let start = Instant::now();
    for row in &mut output.tokens {
        if matches!(row.announce_status.as_deref(), Some("accepted" | "unknown")) { row.wait_timed_out = true; }
    }
    while start.elapsed() < duration && output.tokens.iter().any(|r| r.wait_timed_out) {
        let left = duration.saturating_sub(start.elapsed());
        if tokio::time::timeout(left, round(input, owner, provider, origins, since_ms, output)).await.is_err() {
            for row in output.tokens.iter_mut().filter(|r| r.wait_timed_out) {
                row.intent_status.get_or_insert_with(|| "unknown".into());
                row.status_error = Some("status read exceeded --wait".into());
            }
            return;
        }
        if !output.tokens.iter().any(|r| r.wait_timed_out) { return; }
        tokio::time::sleep(POLL.min(duration.saturating_sub(start.elapsed()))).await;
    }
}

async fn round(input: &Input, owner: Address, provider: &DynProvider, origins: &Origins<'_>, since_ms: u64, output: &mut Output) {
    let Ok(config) = evm::chain_config(&input.chain_id) else { return; };
    let indexed: BTreeMap<B256, (String, u64)> = intentscan::history(origins, owner, config.id, since_ms).await
        .map(|intents| intents.into_iter().map(|i| (i.id, (i.status, i.deadline_ms))).collect()).unwrap_or_default();
    let now = now_ms();
    let mut unresolved = Vec::new();
    for (index, row) in output.tokens.iter_mut().enumerate().filter(|(_, r)| r.wait_timed_out) {
        let id = row.intent_id.as_deref().and_then(|id| id.parse::<B256>().ok());
        // The index can list a fill late, so its "expired" stands only when this round's lens agrees.
        let listed = id.and_then(|id| indexed.get(&id));
        if listed.is_some() && row.announce_status.as_deref() == Some("unknown") {
            row.announce_status = Some("accepted".into()); row.outcome = "placed".into();
        }
        match listed {
            Some((status, _)) if terminal(status) && status != "expired" => record(row, status),
            Some((status, deadline)) if status == "open" && now <= *deadline => record(row, status),
            _ => unresolved.push(index),
        }
    }
    if !unresolved.is_empty() { chain(provider, config, unresolved, output).await; }
}

/// One lens read for every intent the index did not settle: missing, unreachable, expired, or
/// still open after its deadline.
async fn chain(provider: &DynProvider, config: evm::ChainConfig, unresolved: Vec<usize>, output: &mut Output) {
    let (known, missing): (Vec<usize>, Vec<usize>) = unresolved.into_iter().partition(|&i| output.tokens[i].order.is_some());
    for index in missing { fail(&mut output.tokens[index], "signed order unavailable"); }
    if known.is_empty() { return; }
    let orders = known.iter().filter_map(|&i| output.tokens[i].order.clone()).collect();
    match intent::preview_statuses(provider, config, orders).await {
        Ok(statuses) if statuses.len() == known.len() => {
            for (index, status) in known.into_iter().zip(statuses) {
                let row = &mut output.tokens[index];
                // A timed-out publish the index never listed and the lens finds expired was never live.
                let unseen = status == "expired" && row.announce_status.as_deref() == Some("unknown");
                record(row, if unseen { "not_placed" } else { &status });
            }
        }
        Ok(_) => for index in known { fail(&mut output.tokens[index], "lens returned a different number of intents"); },
        Err(error) => {
            let text = crate::redact::urls(&error.to_string());
            for index in known { fail(&mut output.tokens[index], &text); }
        }
    }
}

fn record(row: &mut Row, status: &str) {
    row.wait_timed_out = !terminal(status);
    row.intent_status = Some(status.into());
    row.status_error = None;
}

fn fail(row: &mut Row, error: &str) {
    row.intent_status = Some("unknown".into());
    row.status_error = Some(error.into());
}

/// Index-first rounds, the batched lens fallback, and an early stop once every intent is terminal.
#[cfg(test)]
pub(super) mod tests {
    use super::super::{Input, Output, Row, status};
    use crate::{evm, order_types::{self, IntentLensV3}, service::{intentscan::{Origins, fixture}, test_http::TestHttp, test_rpc::{TestRpc, ok}}};
    use alloy::{primitives::{Address, U256}, sol_types::{SolCall, SolValue}};
    use serde_json::json;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    fn input(wait: u64) -> Input {
        serde_json::from_value(json!({"chain_id":"8453", "proxy":Address::repeat_byte(6), "receive":"WETH", "tokens":[],
            "max_usd":"5", "max_loss_bps":100, "wait":wait})).unwrap()
    }

    fn owner() -> Address { Address::repeat_byte(4) }

    fn rows(count: u8) -> (Output, Vec<order_types::Order>) {
        let orders: Vec<_> = (1..=count).map(|i| fixture::order(owner(), Address::repeat_byte(i), 10, Address::repeat_byte(3), 5, u64::from(i))).collect();
        let tokens = orders.iter().map(|order| {
            let mut row = Row::new(order.tokenIn.to_string());
            row.intent_id = Some(format!("{:?}", order_types::order_id(order)));
            row.announce_status = Some("accepted".into());
            row.order = Some(order.clone());
            row
        }).collect();
        (Output { dry_run: false, owner: owner().to_string(), note: None, tokens }, orders)
    }

    /// Lens RPC answering every order with `in_window` false (expired) unless `filled`; counts batched reads.
    fn lens(filled: bool, reads: Arc<AtomicUsize>) -> TestRpc {
        TestRpc::start(move |body| {
            let data = hex::decode(body["params"][0]["input"].as_str().or(body["params"][0]["data"].as_str()).unwrap().trim_start_matches("0x")).unwrap();
            if data.starts_with(&IntentLensV3::PREVIEW_LAYOUTCall::SELECTOR) { return Some(ok(body, json!(format!("0x{}", hex::encode(U256::from(3).abi_encode()))))); }
            reads.fetch_add(1, Ordering::SeqCst);
            let count = IntentLensV3::previewManyCall::abi_decode(&data).unwrap().o.len();
            let views = vec![fixture::view(filled, false, false); count];
            Some(ok(body, json!(format!("0x{}", hex::encode(IntentLensV3::previewManyCall::abi_encode_returns(&views))))))
        })
    }

    #[tokio::test]
    async fn wait_reads_the_index_and_stops_when_every_intent_is_terminal() {
        let (mut output, orders) = rows(2);
        let page = fixture::page(vec![fixture::item(8453, &orders[0], Address::repeat_byte(2), 1, "filled", 5_000, u64::MAX),
            fixture::item(8453, &orders[1], Address::repeat_byte(2), 1, "cancelled", 5_000, u64::MAX)], None);
        let index = TestHttp::start(move |_| (200, page.clone()));
        let reads = Arc::new(AtomicUsize::new(0));
        let rpc = lens(true, reads.clone());
        let started = std::time::Instant::now();
        status::wait(&input(30), owner(), &evm::read_provider(&rpc.url).unwrap(), &Origins { stream: &index.url, data: "http://127.0.0.1:1" }, 1_000, &mut output).await;
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "the wait must end once all intents are terminal");
        let statuses: Vec<_> = output.tokens.iter().map(|r| (r.intent_status.as_deref(), r.wait_timed_out, r.status_error.as_deref())).collect();
        assert_eq!(statuses, vec![(Some("filled"), false, None), (Some("cancelled"), false, None)]);
        assert_eq!(index.targets().len(), 1, "one history read per round");
        assert!(index.targets()[0].starts_with(&format!("/v1/intents?owner={:?}&chain_ids=8453", owner())));
        assert_eq!(rpc.called("eth_call"), 0, "the index settled every intent");
    }

    #[tokio::test]
    async fn wait_falls_back_to_one_batched_lens_read_when_the_index_is_down() {
        let (mut output, _) = rows(3);
        let reads = Arc::new(AtomicUsize::new(0));
        let rpc = lens(true, reads.clone());
        let down = Origins { stream: "http://127.0.0.1:1", data: "http://127.0.0.1:1" };
        status::wait(&input(30), owner(), &evm::read_provider(&rpc.url).unwrap(), &down, 1_000, &mut output).await;
        assert!(output.tokens.iter().all(|r| r.intent_status.as_deref() == Some("filled") && !r.wait_timed_out));
        assert_eq!(reads.load(Ordering::SeqCst), 1, "all three intents in one previewMany");
        assert_eq!(rpc.called("eth_getLogs"), 0);
    }

    #[tokio::test]
    async fn wait_keeps_open_intents_and_asks_the_lens_about_missing_or_overdue_ones() {
        let (mut output, orders) = rows(3);
        let now = status::now_ms();
        let page = fixture::page(vec![fixture::item(8453, &orders[0], Address::repeat_byte(2), 1, "open", 5_000, now + 600_000),
            fixture::item(8453, &orders[1], Address::repeat_byte(2), 1, "open", 5_000, now - 1)], None);
        let index = TestHttp::start(move |_| (200, page.clone()));
        let reads = Arc::new(AtomicUsize::new(0));
        let rpc = lens(false, reads.clone());
        status::wait(&input(1), owner(), &evm::read_provider(&rpc.url).unwrap(), &Origins { stream: &index.url, data: "http://127.0.0.1:1" }, 1_000, &mut output).await;
        let statuses: Vec<_> = output.tokens.iter().map(|r| (r.intent_status.as_deref(), r.wait_timed_out)).collect();
        assert_eq!(statuses, vec![(Some("open"), true), (Some("expired"), false), (Some("expired"), false)]);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn timed_out_publish_is_polled_and_ends_not_placed_when_never_listed() {
        let (mut output, _) = rows(1);
        output.tokens[0].announce_status = Some("unknown".into());
        let index = TestHttp::start(|_| (200, fixture::page(Vec::new(), None)));
        let reads = Arc::new(AtomicUsize::new(0));
        let rpc = lens(false, reads.clone());
        status::wait(&input(30), owner(), &evm::read_provider(&rpc.url).unwrap(), &Origins { stream: &index.url, data: "http://127.0.0.1:1" }, 1_000, &mut output).await;
        let row = &output.tokens[0];
        assert_eq!((row.intent_status.as_deref(), row.wait_timed_out, row.announce_status.as_deref()), (Some("not_placed"), false, Some("unknown")));
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn index_expired_is_final_only_when_the_lens_agrees() {
        for (lens_filled, expected) in [(true, "filled"), (false, "expired")] {
            let (mut output, orders) = rows(1);
            let page = fixture::page(vec![fixture::item(8453, &orders[0], Address::repeat_byte(2), 1, "expired", 5_000, 6_000)], None);
            let index = TestHttp::start(move |_| (200, page.clone()));
            let reads = Arc::new(AtomicUsize::new(0));
            let rpc = lens(lens_filled, reads.clone());
            status::wait(&input(30), owner(), &evm::read_provider(&rpc.url).unwrap(), &Origins { stream: &index.url, data: "http://127.0.0.1:1" }, 1_000, &mut output).await;
            let row = &output.tokens[0];
            assert_eq!((row.intent_status.as_deref(), row.wait_timed_out), (Some(expected), false));
            assert_eq!(reads.load(Ordering::SeqCst), 1, "one lens read settles the index's expired");
        }
    }
}
