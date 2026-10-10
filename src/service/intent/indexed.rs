// Intent reads that need no IntentAnnounced log: orders come from the intent index, so intents
// published only to the off-chain stream resolve, and live fields come from the lens. The settler
// answers by id alone when neither the index nor a log holds the order.
use super::IntentRecord;
use super::read::{record, record_from};
use crate::evm::ChainConfig;
use crate::order_types::{IntentLensV3, IntentSettlerV3};
use crate::service::intentscan::{self, Origins};
use alloy::primitives::{Address, B256};
use alloy::providers::DynProvider;
use eyre::{Result, ensure};

/// Orders per batched lens read when listing.
const PREVIEW_BATCH: usize = 100;

pub(super) async fn status(origins: &Origins<'_>, provider: &DynProvider, config: ChainConfig, id: B256) -> Result<Option<IntentRecord>> {
    let Some(intent) = intentscan::intent(origins, config.id, id).await? else { return Ok(None) };
    Ok(Some(record(provider, config, intent.order, intent.agent.map(|(agent, _)| agent)).await?))
}

/// The owner's intents in the index, optionally only those `agent` signed, newest first.
pub(super) async fn list(origins: &Origins<'_>, provider: &DynProvider, config: ChainConfig, owner: Address, agent: Option<Address>) -> Result<Vec<IntentRecord>> {
    let intents: Vec<_> = intentscan::history(origins, owner, config.id, 0).await?.into_iter()
        .filter(|intent| agent.is_none() || intent.agent.map(|(signer, _)| signer) == agent).collect();
    let lens = IntentLensV3::new(config.lens, provider.clone());
    let mut out = Vec::new();
    for chunk in intents.chunks(PREVIEW_BATCH) {
        let views = lens.previewMany(chunk.iter().map(|intent| intent.order.clone()).collect()).call().await?;
        ensure!(views.len() == chunk.len(), "lens returned {} views for {} intents", views.len(), chunk.len());
        out.extend(chunk.iter().zip(&views).map(|(intent, view)| record_from(view, &intent.order, intent.agent.map(|(a, _)| a))));
    }
    Ok(out)
}

/// The settler's filled or cancelled state for an intent whose order is unavailable; fields that
/// need the order are empty.
pub(super) async fn settled(provider: &DynProvider, config: ChainConfig, id: B256) -> Result<IntentRecord> {
    let settler = IntentSettlerV3::new(config.settler, provider.clone());
    let (filled, cancelled) = (settler.filled(id).call().await?, settler.cancelled(id).call().await?);
    ensure!(filled || cancelled, "intent {id:?} was not found in the intent index or IntentAnnounced logs, and the settler marks it neither filled nor cancelled");
    let status = if filled { "filled" } else { "cancelled" };
    Ok(IntentRecord { id: format!("{id:?}"), placed_by: String::new(), owner: String::new(), agent: None, pair: String::new(),
        amount_in: String::new(), start_out: String::new(), end_out: String::new(), window: String::new(), exclusive_window: false,
        floor_now: String::new(), fee_now: String::new(), required_now: String::new(), floor_for_outsider: String::new(),
        required_for_outsider: String::new(), status: status.into(),
        reason: format!("settler marked {status}; the order is unavailable without the intent index or an IntentAnnounced log") })
}

/// Stream-only intents (no IntentAnnounced log) through `intent status` and `intent list`.
#[cfg(test)]
mod tests {
    use super::super::{ListInput, StatusInput, read::{list_from, status_from}};
    use crate::{evm, order_types::{self, IntentLensV3, IntentSettlerV3}, service::{intentscan::{Origins, fixture}, test_http::TestHttp, test_rpc::{TestRpc, ok}}};
    use alloy::{primitives::{Address, U256}, sol_types::{SolCall, SolValue}};
    use serde_json::json;

    /// Lens answers every order with `view`; the settler marks `filled`; logs are empty.
    fn chain(view: IntentLensV3::IntentView, filled: bool) -> TestRpc {
        TestRpc::start(move |body| Some(ok(body, match body["method"].as_str().unwrap() {
            "eth_blockNumber" => json!("0x100"),
            "eth_getLogs" => json!([]),
            _ => {
                let data = hex::decode(body["params"][0]["input"].as_str().or(body["params"][0]["data"].as_str()).unwrap().trim_start_matches("0x")).unwrap();
                let reply = match &data[..4] {
                    s if s == IntentLensV3::PREVIEW_LAYOUTCall::SELECTOR => U256::from(3).abi_encode(),
                    s if s == IntentLensV3::previewCall::SELECTOR => IntentLensV3::previewCall::abi_encode_returns(&view),
                    s if s == IntentLensV3::previewManyCall::SELECTOR =>
                        IntentLensV3::previewManyCall::abi_encode_returns(&vec![view.clone(); IntentLensV3::previewManyCall::abi_decode(&data).unwrap().o.len()]),
                    s if s == IntentSettlerV3::filledCall::SELECTOR => filled.abi_encode(),
                    s if s == IntentSettlerV3::cancelledCall::SELECTOR => false.abi_encode(),
                    other => panic!("unexpected selector {}", hex::encode(other)),
                };
                json!(format!("0x{}", hex::encode(reply)))
            }
        })))
    }

    fn order(nonce: u64) -> order_types::Order { fixture::order(Address::repeat_byte(4), Address::repeat_byte(1), 10, Address::repeat_byte(3), 5, nonce) }

    fn index(orders: Vec<order_types::Order>) -> TestHttp {
        TestHttp::start(move |target| {
            let items: Vec<_> = orders.iter().map(|o| fixture::item(8453, o, Address::repeat_byte(2), 1, "open", 1_000, 2_000)).collect();
            if target.starts_with("/v1/intents?") { return (200, fixture::page(items, None)); }
            match orders.iter().position(|o| target == format!("/v1/intent/{:?}", order_types::order_id(o))) {
                Some(i) => (200, json!({"intents": [items[i]]}).to_string()),
                None => (404, r#"{"error":"not_found"}"#.into()),
            }
        })
    }

    fn input(order: &order_types::Order) -> StatusInput {
        StatusInput { chain_id: "8453".into(), id: format!("{:?}", order_types::order_id(order)), lookback_blocks: Some(10) }
    }

    #[tokio::test]
    async fn status_of_a_stream_only_intent_comes_from_the_index_and_the_lens() {
        let config = evm::chain_config("8453").unwrap();
        let server = index(vec![order(1)]);
        let origins = Origins { stream: &server.url, data: &server.url };
        for (view, expected) in [(fixture::view(true, false, true), "filled"), (fixture::view(false, false, false), "expired")] {
            let rpc = chain(view, false);
            let record = status_from(&origins, &evm::read_provider(&rpc.url).unwrap(), config, &input(&order(1))).await.unwrap();
            assert_eq!((record.status.as_str(), record.agent.clone(), record.amount_in.as_str()), (expected, Some(format!("{:?}", Address::repeat_byte(2))), "10"));
            assert_eq!(rpc.called("eth_getLogs"), 0, "no IntentAnnounced scan");
        }
    }

    #[tokio::test]
    async fn status_without_index_or_announce_log_falls_back_to_the_settler() {
        let config = evm::chain_config("8453").unwrap();
        let down = Origins { stream: "http://127.0.0.1:1", data: "http://127.0.0.1:1" };
        let rpc = chain(fixture::view(true, false, true), true);
        let record = status_from(&down, &evm::read_provider(&rpc.url).unwrap(), config, &input(&order(1))).await.unwrap();
        assert_eq!((record.status.as_str(), record.owner.as_str()), ("filled", ""));
        assert!(record.reason.contains("settler marked filled"), "{}", record.reason);
        let unknown = chain(fixture::view(true, false, true), false);
        let error = status_from(&down, &evm::read_provider(&unknown.url).unwrap(), config, &input(&order(1))).await.unwrap_err();
        assert!(error.to_string().contains("neither filled nor cancelled"), "{error}");
    }

    #[tokio::test]
    async fn owner_list_reads_the_index_and_one_batched_lens_call() {
        let config = evm::chain_config("8453").unwrap();
        let server = index(vec![order(1), order(2)]);
        let origins = Origins { stream: &server.url, data: &server.url };
        let rpc = chain(fixture::view(false, false, true), false);
        let input = ListInput { chain_id: "8453".into(), owner: Some(Address::repeat_byte(4).to_string()), agent: None, lookback_blocks: None };
        let records = list_from(&origins, &evm::read_provider(&rpc.url).unwrap(), config, &input).await.unwrap();
        assert_eq!(records.iter().map(|r| r.status.as_str()).collect::<Vec<_>>(), ["open", "open"]);
        assert_eq!((rpc.called("eth_call"), rpc.called("eth_getLogs")), (2, 0), "layout plus one previewMany");
    }
}
