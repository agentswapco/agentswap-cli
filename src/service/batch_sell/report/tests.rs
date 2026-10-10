// Batch-sale report from a mocked request, one mocked multicall grant read, mocked app prices and
// a mocked intent index: sold, partly sold, expired and not placed rows, discounts and next steps.
use super::{build, next, models::GrantState, rows::{self, discount_bps, pct}, text};
use crate::{order_types::UserProxyV6, service::{intentscan::{Origins, fixture}, portfolio::discovery::BalanceReader, test_http::TestHttp, test_rpc::{TestRpc, ok}, token}};
use crate::order_types::Erc20Metadata;
use alloy::{primitives::{Address, B256, U256}, providers::{MULTICALL3_ADDRESS, bindings::IMulticall3}, sol_types::SolCall};
use serde_json::{Value, json};

const ID: &str = "abcdefghijklmnopqrstuv";
const NOW: u64 = 1_800_000_000;
const E18: u64 = 1_000_000_000_000_000_000;

fn owner() -> Address { Address::repeat_byte(4) }
fn agent() -> Address { Address::repeat_byte(2) }
fn proxy() -> Address { Address::repeat_byte(6) }
fn usdc() -> Address { token::from_registry("USDC", 8453).unwrap().address.parse().unwrap() }
fn spend() -> [Address; 4] { [0x11, 0x22, 0x33, 0x44].map(Address::repeat_byte) }

#[test]
fn discount_math_is_signed_basis_points_rendered_as_percent() {
    let u = |v: u64| U256::from(v);
    assert_eq!(discount_bps(u(1000), u(990)), Some(100));
    assert_eq!(discount_bps(u(1000), u(1010)), Some(-100));
    assert_eq!(discount_bps(u(3), u(2)), Some(3333));
    assert_eq!(discount_bps(U256::ZERO, u(5)), None);
    assert_eq!(discount_bps(u(1), U256::MAX), None, "an absurd ratio is unreported, not wrapped");
    assert_eq!([pct(100), pct(-100), pct(3333), pct(5), pct(-5), pct(0)], ["1.00", "-1.00", "33.33", "0.05", "-0.05", "0.00"]);
}

fn record() -> crate::service::batch_sell::models::Record {
    let mut tokens: Vec<Value> = spend().iter().zip(["100", "50", "10", "5"]).map(|(a, cap)| json!({"address": a, "cap": cap})).collect();
    tokens.push(json!({"address": usdc(), "cap": "0"}));
    serde_json::from_value(json!({"id": ID, "status": "confirmed", "createdAt": "2027-01-15T07:00:00Z",
        "confirmed": {"proxy": proxy(), "generation": "1", "maxLossBps": 300, "confirmedAt": "2027-01-15T07:10:00Z"},
        "request": {"v": 1, "chainId": 8453, "agent": agent(), "owner": owner(), "purpose": "batch-sell", "maxLossBps": 500, "tokens": tokens}})).unwrap()
}

/// (cap, used, balance) per spend token, in whole 18-decimal units.
const BUDGETS: [(u64, u64, u64); 4] = [(100, 100, 0), (50, 20, 30), (10, 0, 10), (5, 0, 5)];

fn answer(call: &IMulticall3::Call3) -> Vec<u8> {
    let data = &call.callData;
    let token = spend().iter().position(|a| *a == call.target);
    let e18 = |v: u64| U256::from(v) * U256::from(E18);
    match &data[..4] {
        s if s == UserProxyV6::ownerCall::SELECTOR => UserProxyV6::ownerCall::abi_encode_returns(&owner()),
        s if s == UserProxyV6::policyOfCall::SELECTOR => UserProxyV6::policyOfCall::abi_encode_returns(&UserProxyV6::policyOfReturn { expiry: NOW + 3600, epochLen: 604_800, actionMask: 5, generation: 1 }),
        s if s == IMulticall3::getCurrentBlockTimestampCall::SELECTOR => IMulticall3::getCurrentBlockTimestampCall::abi_encode_returns(&U256::from(NOW)),
        s if s == UserProxyV6::agentTokenInfoCall::SELECTOR => {
            let asked = UserProxyV6::agentTokenInfoCall::abi_decode(data).unwrap().token;
            let (cap, used, _) = spend().iter().position(|a| *a == asked).map_or((0, 0, 0), |i| BUDGETS[i]);
            UserProxyV6::agentTokenInfoCall::abi_encode_returns(&UserProxyV6::agentTokenInfoReturn { allowed: true, cap: e18(cap), used: e18(used), epochStart: 1 })
        }
        s if s == Erc20Metadata::decimalsCall::SELECTOR => Erc20Metadata::decimalsCall::abi_encode_returns(&if token.is_some() { 18 } else { 6 }),
        s if s == Erc20Metadata::symbolCall::SELECTOR => Erc20Metadata::symbolCall::abi_encode_returns(&["AAA", "BBB", "CCC", "DDD", "USDC"][token.unwrap_or(4)].to_string()),
        s if s == BalanceReader::balanceOfCall::SELECTOR => BalanceReader::balanceOfCall::abi_encode_returns(&token.map_or(U256::ZERO, |i| e18(BUDGETS[i].2))),
        other => panic!("unexpected selector {}", hex::encode(other)),
    }
}

fn chain() -> TestRpc {
    TestRpc::start(|body| {
        assert_eq!(body["method"], "eth_call", "the report reads the chain only through one multicall");
        let tx = &body["params"][0];
        assert_eq!(tx["to"].as_str().unwrap().parse::<Address>().unwrap(), MULTICALL3_ADDRESS);
        let data = hex::decode(tx["input"].as_str().or(tx["data"].as_str()).unwrap().trim_start_matches("0x")).unwrap();
        let calls = IMulticall3::aggregate3Call::abi_decode(&data).unwrap().calls;
        let results: Vec<_> = calls.iter().map(|c| IMulticall3::Result { success: true, returnData: answer(c).into() }).collect();
        Some(ok(body, json!(format!("0x{}", hex::encode(IMulticall3::aggregate3Call::abi_encode_returns(&results))))))
    })
}

fn app() -> TestHttp {
    let [a, b, c, _] = spend();
    let prices = json!({"prices": {a.to_string(): {"priceUsd": 2, "source": "defillama"}, b.to_string(): {"priceUsd": 0.5, "source": "defillama"},
        c.to_string(): {"priceUsd": 1, "source": "defillama"}, usdc().to_string(): {"priceUsd": 1, "source": "defillama"}}}).to_string();
    TestHttp::start(move |target| { assert!(target.starts_with("/api/prices?chainId=8453"), "{target}"); (200, prices.clone()) })
}

const CREATED: u64 = 1_799_997_600_000;

/// The intent index with the first `keep` history items: sold, part, lapsed, expired, then three
/// that do not match this sale.
fn index(keep: usize) -> TestHttp {
    let [a, b, c, _] = spend();
    let e = |v: u64| U256::from(v) * U256::from(E18);
    let order = |token, amount, out, nonce| { let mut o = fixture::order(owner(), token, 1, out, 1, nonce); o.amountIn = amount; o };
    let (sold, part, lapsed, expired) = (order(a, e(100), usdc(), 1), order(b, e(20), usdc(), 2), order(b, e(30), usdc(), 3), order(c, e(10), usdc(), 4));
    let (other_agent, other_generation, other_out) = (order(c, e(10), usdc(), 5), order(c, e(10), usdc(), 6), order(c, e(10), Address::repeat_byte(9), 7));
    let item = |o, who, generation, status, deadline| fixture::item(8453, o, who, generation, status, CREATED, deadline);
    let page = fixture::page(vec![item(&sold, agent(), 1, "filled", CREATED + 600_000), item(&part, agent(), 1, "filled", CREATED + 600_000),
        item(&lapsed, agent(), 1, "open", CREATED + 600_000), item(&expired, agent(), 1, "expired", CREATED + 600_000),
        item(&other_agent, Address::repeat_byte(8), 1, "filled", CREATED), item(&other_generation, agent(), 2, "filled", CREATED),
        item(&other_out, agent(), 1, "filled", CREATED)].into_iter().take(keep).collect(), None);
    let id = |o: &crate::order_types::Order| crate::order_types::order_id(o);
    let fills = [(id(&sold), fixture::fill(id(&sold), "196000000", B256::repeat_byte(0xa1), 1_799_998_000_000)),
        (id(&part), fixture::fill(id(&part), "10100000", B256::repeat_byte(0xb1), 1_799_998_100_000))];
    TestHttp::start(move |target| {
        if target.starts_with("/v1/intents?") { return (200, page.clone()); }
        fills.iter().find(|(id, _)| target == format!("/v1/intent/{id:?}")).map_or((404, "{}".into()), |(_, body)| (200, body.clone()))
    })
}

#[tokio::test]
async fn report_rows_summary_and_next_steps_from_mocked_sources() {
    let (rpc, app, index) = (chain(), app(), index(7));
    let origins = Origins { stream: &index.url, data: &index.url };
    let report = build(record(), &crate::evm::read_provider(&rpc.url).unwrap(), &app.url, &origins).await.unwrap();
    assert_eq!(rpc.called("eth_call"), 1, "one batched chain read");
    assert_eq!(rpc.called("eth_getLogs"), 0);
    let targets = index.targets();
    assert_eq!(targets.len(), 3, "one history page and one fill record per matching fill: {targets:?}");
    assert_eq!(targets[0], format!("/v1/intents?owner={:?}&chain_ids=8453&limit=500", owner()));
    let value = serde_json::to_value(&report).unwrap();
    let expected: Value = serde_json::from_str(&include_str!("tests_expected.json").replace("{USDC}", &usdc().to_string())).unwrap();
    assert_eq!(value, expected, "{}", serde_json::to_string_pretty(&value).unwrap());
    assert_eq!(text(&report), include_str!("tests_expected.txt"), "{}", text(&report));
}

#[tokio::test]
async fn report_without_the_index_uses_grant_budgets_and_says_proceeds_are_unknown() {
    let (rpc, app) = (chain(), app());
    let down = Origins { stream: "http://127.0.0.1:1", data: "http://127.0.0.1:1" };
    let report = build(record(), &crate::evm::read_provider(&rpc.url).unwrap(), &app.url, &down).await.unwrap();
    let e = |v: u64| (U256::from(v) * U256::from(E18)).to_string();
    let statuses: Vec<_> = report.tokens.iter().map(|r| (r.status.clone(), r.sold_raw.clone(), r.received_raw.is_none())).collect();
    assert_eq!(statuses, vec![("sold".into(), e(100), true), ("partly_sold".into(), e(20), true), ("not_placed".into(), "0".into(), true), ("not_placed".into(), "0".into(), true)]);
    assert!(report.warnings[0].starts_with("intent records unavailable"), "{:?}", report.warnings);
    let s = &report.summary;
    assert_eq!((s.received_raw.as_str(), s.known_proceeds_tokens, s.unknown_proceeds_tokens, s.unknown_proceeds_value_usd.as_str()), ("0", 0, 2, "210"));
    assert_eq!(report.summary.average_discount_pct, None);
    let text = text(&report);
    assert!(!text.lines().any(|l| l.starts_with("Received")) && text.contains("Proceeds unknown for 2 tokens ($210.00 sold): no fill record."), "{text}");
}

#[tokio::test]
async fn market_sold_and_index_filled_rows_split_known_and_unknown_proceeds() {
    let (rpc, app, index) = (chain(), app(), index(1));
    let origins = Origins { stream: &index.url, data: &index.url };
    let report = build(record(), &crate::evm::read_provider(&rpc.url).unwrap(), &app.url, &origins).await.unwrap();
    let s = &report.summary;
    assert_eq!((s.tokens_sold, s.received_raw.as_str(), s.sold_value_usd.as_str()), (2, "196000000", "210"));
    assert_eq!((s.known_proceeds_tokens, s.known_proceeds_value_usd.as_str()), (1, "200"));
    assert_eq!((s.unknown_proceeds_tokens, s.unknown_proceeds_value_usd.as_str()), (1, "10"));
    let text = text(&report);
    assert!(text.contains("Sold 2/4 tokens worth $210.00 at independent prices.\nReceived 196 USDC ($196.00) for $200.00 sold with fill records.\n\
        Proceeds unknown for 1 token ($10.00 sold): no fill record.\n"), "{text}");
}

#[test]
fn zero_unsold_from_an_empty_balance_is_labelled() {
    let token = rows::Token { address: spend()[0], symbol: "AAA".into(), decimals: 0, cap: U256::from(10), sold: U256::from(4), balance: U256::ZERO };
    let map = std::collections::BTreeMap::new();
    let prices = rows::Prices { map: &map, receive: usdc(), receive_decimals: 6 };
    let (mut tally, mut warnings) = (rows::Tally::default(), Vec::new());
    let row = rows::row(&token, &[], &prices, &mut tally, &mut warnings);
    assert_eq!((row.status.as_str(), row.unsold_raw.as_str()), ("sold", "0"));
    assert!(warnings.contains(&"AAA: 6 of the cap was not sold; the owner holds none of this token, so none is left to sell".to_string()), "{warnings:?}");
}

#[tokio::test]
async fn report_refuses_an_unconfirmed_request_before_any_read() {
    let mut pending = record();
    pending.status = "pending".into();
    pending.confirmed = None;
    let provider = crate::evm::read_provider("http://127.0.0.1:1").unwrap();
    let error = build(pending, &provider, "http://127.0.0.1:1", &Origins::public()).await.unwrap_err();
    assert!(error.to_string().contains("not confirmed"), "{error}");
}

#[test]
fn open_intents_keep_the_row_open_and_ask_for_a_later_report() {
    let order = fixture::order(owner(), spend()[0], 7, usdc(), 1, 1);
    let open = (crate::service::intentscan::Intent { id: crate::order_types::order_id(&order), order, agent: Some((agent(), 1)),
        status: "open".into(), deadline_ms: NOW * 1000 + 300_000 }, None);
    let token = rows::Token { address: spend()[0], symbol: "AAA".into(), decimals: 18, cap: U256::from(7), sold: U256::ZERO, balance: U256::from(7) };
    let map = std::collections::BTreeMap::new();
    let prices = rows::Prices { map: &map, receive: usdc(), receive_decimals: 6 };
    let (mut tally, mut warnings) = (rows::Tally::default(), Vec::new());
    let row = rows::row(&token, &[&open], &prices, &mut tally, &mut warnings);
    assert_eq!((row.status.as_str(), row.open_until.as_deref(), row.unsold_raw.as_str()), ("open", Some("2027-01-15T08:05:00Z"), "7"));
    let grant = GrantState { expiry: NOW + 60, expires_at: "2027-01-15T08:01:00Z".into(), active: true, generation_matches: true };
    assert_eq!(next(ID, &grant, &tally), vec![
        format!("1 intent is open until 2027-01-15T08:05:00Z; send the owner `batch-sell report --request {ID}` again after that."),
        format!("After the open intents close, run `batch-sell run --request {ID}` (MCP `batch_sell_run`) before the grant expires at 2027-01-15T08:01:00Z to offer the unsold amount again.")]);
    let ended = GrantState { active: false, ..grant };
    assert!(next(ID, &ended, &tally)[1].starts_with("The grant expired at 2027-01-15T08:01:00Z; selling the rest needs a new `batch-sell plan`"));
}
