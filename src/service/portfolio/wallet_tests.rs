// Indexed wallet source selection, price priority and untrusted metadata regressions.
// Counted RPC fixtures prove indexed holdings and quote metadata never require token reads.
use super::*;
use crate::service::test_rpc::{TestRpc, ok};
use alloy::sol_types::SolValue;
use serde_json::json;

#[tokio::test]
async fn wallet_prices_are_display_only_and_app_prices_win() {
    for source in [None, Some("defillama"), Some("1inch")] {
        let app_price = source.is_some();
        let rpc = TestRpc::start(|_| panic!("indexed holdings must not use RPC"));
        let provider = evm::read_provider(&rpc.url).unwrap();
        let address = Address::repeat_byte(1);
        let prices = if app_price { json!({format!("{address:?}"):{"priceUsd":3,"source":source}}) } else { json!({}) };
        let app = http::Server::start(vec![(200, String::new(), json!({"chainId":4663,"owner":Address::repeat_byte(4),
            "indexed":true,"truncated":false,"tokens":[{"address":address,"balanceRaw":"1000000",
                "decimals":6,"symbol":"TEST","name":"Test Token","priceUsd":"2"}]}).to_string()),
            (200, String::new(), json!({"prices":prices}).to_string())]);
        let quote = http::Server::start(vec![(200, String::new(), json!({"output":"123"}).to_string())]);
        let mut request = tests::input("4663"); request.max_usd = Some("3".into()); request.quote_token = Some(format!("{address:?}"));
        let output = read(&Client::new(&quote.url, None), request, &provider, &app.url).await.unwrap();
        let row = &output.tokens[0];
        assert_eq!(row.source.as_deref(), Some(source.unwrap_or("alchemy")));
        assert_eq!(row.price_usd.as_deref(), Some(if app_price { "3" } else { "2" }));
        assert_eq!(row.value_usd, row.price_usd);
        assert_eq!(row.floor_eligible, app_price);
        assert!(row.dust);
        assert_eq!(row.quote_out_raw.as_deref(), Some("123"));
        assert_eq!(rpc.called("eth_call"), 0);
        assert_eq!(rpc.called("eth_getLogs"), 0);
    }
}

#[tokio::test]
async fn indexed_null_metadata_and_untrusted_text_are_not_read_on_chain() {
    for (symbol, name, price) in [("\u{1b}[31mUSD", "bad\u{202e}name", "bad"), ("TEST", "Test Token", "0")] {
        let rpc = TestRpc::start(|_| panic!("null metadata must not trigger RPC"));
        let provider = evm::read_provider(&rpc.url).unwrap();
        let app = http::Server::start(vec![(200, String::new(), json!({"chainId":4663,"owner":Address::repeat_byte(4),
            "indexed":true,"truncated":true,"tokens":[{"address":Address::repeat_byte(1),"balanceRaw":"10",
                "decimals":null,"symbol":symbol,"name":name,"priceUsd":price}]}).to_string()),
            (200, String::new(), json!({"prices":{}}).to_string())]);
        let output = read(&Client::new("http://127.0.0.1:1", None), tests::input("4663"), &provider, &app.url).await.unwrap();
        let row = &output.tokens[0];
        assert_eq!(row.status, "metadata_error");
        assert!(row.decimals.is_none() && row.price_usd.is_none() && row.quote_out_raw.is_none());
        assert!(!row.symbol.contains('\u{1b}'));
        assert_eq!(row.name.as_deref(), (symbol == "TEST").then_some("Test Token"));
        assert!(output.wallet_tokens_truncated);
        assert_eq!(rpc.called("eth_call"), 0);
        assert_eq!(rpc.called("eth_getLogs"), 0);
    }
}

#[tokio::test]
async fn indexed_empty_skips_catalog_but_explicit_missing_token_is_read() {
    for explicit in [false, true] {
        let rpc = tests::fixture(false, false, false, 6);
        let provider = evm::read_provider(&rpc.url).unwrap();
        let app = http::Server::start(vec![(200, String::new(), json!({"chainId":8453,"owner":Address::repeat_byte(4),
            "indexed":true,"truncated":false,"tokens":[]}).to_string()),
            (200, String::new(), json!({"prices":{}}).to_string())]);
        let catalog = http::Server::start(vec![]);
        let mut request = tests::input("8453"); request.max_usd = None;
        if !explicit { request.tokens.clear(); }
        let output = read(&Client::new(&catalog.url, None), request, &provider, &app.url).await.unwrap();
        assert_eq!(output.tokens.len(), usize::from(explicit));
        assert_eq!(rpc.called("eth_call"), if explicit { 3 } else { 0 });
        assert_eq!(rpc.called("eth_getLogs"), 0);
        assert!(catalog.requests.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn unavailable_wallet_uses_catalog_and_registry_without_logs() {
    let rpc = TestRpc::start(|body| Some(ok(body, json!(format!("0x{}", hex::encode(U256::ZERO.abi_encode()))))));
    let provider = evm::read_provider(&rpc.url).unwrap();
    let catalog = http::Server::start(vec![(200, String::new(), json!({format!("{:?}", Address::repeat_byte(2)):{"chain_id":8453}}).to_string())]);
    let output = read(&Client::new(&catalog.url, None), tests::input("8453"), &provider, "unused").await.unwrap();
    assert_eq!(output.wallet_tokens, sources::WalletTokens::Unavailable);
    assert!(output.tokens.is_empty());
    assert_eq!(rpc.called("eth_call"), crate::tokens::registry_addresses(8453).len() + 2);
    assert_eq!(rpc.called("eth_getLogs"), 0);
    assert_eq!(catalog.requests.lock().unwrap().len(), 1);
}
