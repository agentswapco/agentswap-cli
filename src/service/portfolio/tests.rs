// Mocked discovery, balance, quote and valuation tests, with reusable read-only RPC fixtures.
// Exercises provenance union, optional indexer failure and exact dust boundaries.
use super::*;
use crate::service::test_rpc::{TestRpc, ok, failure};
use alloy::sol_types::{SolCall, SolValue};
use serde_json::json;

pub(crate) fn fixture(indexer: bool, live: bool, deployed: bool, decimals: u8) -> TestRpc {
    TestRpc::start(move |body| {
        let token = Address::repeat_byte(1);
        let result = match body["method"].as_str().unwrap_or("") {
            "eth_blockNumber" => json!("0x2711"),
            "eth_getLogs" => {
                assert_eq!(body["params"][0]["topics"][2], json!(format!("0x{:0>64}", hex::encode(Address::repeat_byte(4)))));
                json!([{"address":format!("{token:?}"), "topics":[], "data":"0x", "blockNumber":"0x2710", "transactionIndex":"0x0", "logIndex":"0x0", "blockHash":alloy::primitives::B256::ZERO, "transactionHash":alloy::primitives::B256::ZERO, "removed":false}])
            }
            "alchemy_getTokenBalances" if indexer => json!({"tokenBalances":[{"contractAddress":format!("{token:?}"), "tokenBalance":"0x0"}]}),
            "alchemy_getTokenBalances" => return Some(failure(body, "unsupported")),
            "eth_getCode" => json!(if deployed { "0x01" } else { "0x" }),
            "eth_call" => {
                let tx = &body["params"][0];
                let data = tx["input"].as_str().or(tx["data"].as_str()).unwrap();
                let bytes = hex::decode(data.trim_start_matches("0x")).unwrap();
                let selector = &bytes[..4];
                let encoded = if selector == discovery::BalanceReader::balanceOfCall::SELECTOR {
                    let amount = if tx["to"].as_str().unwrap().eq_ignore_ascii_case(&format!("{token:?}")) { 1_000_000u64 } else { 0 };
                    U256::from(amount).abi_encode()
                } else if selector == order_types::Erc20Metadata::decimalsCall::SELECTOR { U256::from(decimals).abi_encode() }
                else if selector == order_types::Erc20Metadata::symbolCall::SELECTOR { "TEST".to_string().abi_encode() }
                else if selector == order_types::UserProxyFactoryV6::proxyOfCall::SELECTOR { Address::repeat_byte(2).abi_encode() }
                else if selector == order_types::UserProxyV6::policyOfCall::SELECTOR {
                    (U256::from(if live { u64::MAX } else { 0 }), U256::from(604800), U256::from(1), U256::from(7)).abi_encode()
                } else { panic!("unexpected read selector"); };
                json!(format!("0x{}", hex::encode(encoded)))
            }
            _ => return Some(json!({"output":"123"})),
        };
        Some(ok(body, result))
    })
}

pub(crate) fn input(chain: &str) -> Input {
    Input { chain_id: chain.into(), owner: format!("{:?}", Address::repeat_byte(4)), tokens: vec![format!("{:?}", Address::repeat_byte(1))],
        max_usd: Some("1".into()), quote_token: Some(format!("{:?}", Address::repeat_byte(3))), lookback_blocks: Some(5001) }
}

#[tokio::test]
async fn transfer_explicit_indexer_union_and_authoritative_balances() {
    for indexer in [true, false] {
        let rpc = fixture(indexer, false, false, 6);
        let provider = evm::read_provider(&rpc.url).unwrap();
        let output = read(&Client::new(&rpc.url, None), input("4663"), &provider, "unused").await.unwrap();
        assert_eq!((output.from_block, output.to_block), (5000, 10001));
        assert_eq!(output.indexer.is_some(), indexer);
        assert_eq!(rpc.called("eth_getLogs"), 2);
        assert_eq!(output.tokens.len(), 1);
        let row = &output.tokens[0];
        assert_eq!(row.balance_raw.as_deref(), Some("1000000"));
        assert_eq!(row.price_usd, None);
        assert_eq!(row.quote_out_raw.as_deref(), Some("123"));
        assert!(!row.dust);
        assert_eq!(row.sources.len(), if indexer { 3 } else { 2 });
        assert!(row.sources.contains(&"transfer".into()));
        assert!(row.sources.contains(&"explicit".into()));
    }
}

#[tokio::test]
async fn exact_price_boundary_unpriced_and_quote_errors() {
    let rpc = fixture(false, false, false, 6);
    let provider = evm::read_provider(&rpc.url).unwrap();
    for (price, threshold, dust, quoted) in [("1", "1", true, true), ("1.000000000000000001", "1", false, false)] {
        let price = price.to_string();
        let pricing = TestRpc::start(move |_| Some(serde_json::from_str(&format!(r#"{{"coins":{{"base:{:?}":{{"price":{price}}}}}}}"#, Address::repeat_byte(1))).unwrap()));
        let mut input = input("8453");
        input.max_usd = Some(threshold.into());
        let output = read(&Client::new(&rpc.url, None), input, &provider, &pricing.url).await.unwrap();
        assert_eq!(output.tokens.len(), 1);
        assert_eq!(output.tokens[0].dust, dust);
        assert_eq!(output.tokens[0].quote_out_raw.is_some(), quoted);
    }
    let output = read(&Client::new("http://127.0.0.1:1", None), input("4663"), &provider, "unused").await.unwrap();
    assert_eq!(output.tokens[0].status, "no_route");
    assert!(!output.tokens[0].dust);
}

#[test]
fn discovery_validates_chains_native_and_registry_union() {
    assert!(discovery::config("5042").is_err());
    for chain in ["8453", "42161", "56", "4663"] { assert!(discovery::config(chain).is_ok()); }
    assert!(discovery::erc20(&Address::ZERO.to_string()).is_err());
    assert!(discovery::erc20(&Address::repeat_byte(0xee).to_string()).is_err());
    let addresses = crate::tokens::registry_addresses(8453);
    assert!(!addresses.is_empty());
    let mut candidates = discovery::Candidates::new();
    let address = discovery::erc20(addresses[0]).unwrap();
    for source in ["registry", "explicit", "transfer", "transfer"] { discovery::add(&mut candidates, address, source); }
    assert_eq!(candidates[&address].len(), 3);
}

#[tokio::test]
async fn malformed_indexer_is_optional_and_invalid_tokens_fail_early() {
    let rpc = TestRpc::start(|body| Some(ok(body, match body["method"].as_str().unwrap() {
        "eth_blockNumber" => json!("0x10"),
        "eth_getLogs" => json!([]),
        "alchemy_getTokenBalances" => json!({"tokenBalances":[{"contractAddress":"bad"}]}),
        _ => panic!("unexpected RPC request"),
    })));
    let provider = evm::read_provider(&rpc.url).unwrap();
    let config = discovery::config("4663").unwrap();
    let (tokens, _, _, indexer) = discovery::discover(&provider, config, Address::repeat_byte(4), &[], Some(1)).await.unwrap();
    assert!(tokens.is_empty());
    assert!(indexer.is_none());
    assert!(discovery::discover(&provider, config, Address::repeat_byte(4), &["invalid".into()], None).await.is_err());
}

#[tokio::test]
async fn failed_balance_is_visible_and_never_quoted() {
    let rpc = TestRpc::start(|body| Some(failure(body, "execution reverted")));
    let provider = evm::read_provider(&rpc.url).unwrap();
    let row = read_row(&provider, 4663, Address::repeat_byte(4), Address::repeat_byte(1), vec!["explicit".into()]).await.unwrap();
    assert_eq!(row.status, "balance_error");
    assert!(row.balance_raw.is_none());
    assert!(row.decimals.is_none());
    assert!(!row.dust);
}
