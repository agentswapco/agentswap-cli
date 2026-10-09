// FIX-round regressions for entrypoint boundaries and published command contracts.
// Source assertions cover extraction and origin ownership; runtime tests cover refusal remedies.
use super::*;
#[path = "service/test_rpc.rs"]
mod test_rpc;
#[path = "client/test_server.rs"]
mod http;

#[test]
fn fix_entrypoints_fit_source_limit() {
    for (name, source) in [("main", include_str!("main.rs")), ("mcp", include_str!("mcp.rs"))] {
        assert!(source.lines().count() <= 300, "{name}: {}", source.lines().count());
    }
}

#[test]
fn fix_meta_origin_is_owned_by_routes() {
    assert!(include_str!("routes.rs").contains("https://meta-api.agentswap.co"));
    assert_eq!(routes::META_ORIGIN, "https://meta-api.agentswap.co");
    assert!(!include_str!("client/quote_backend.rs").contains("https://meta-api.agentswap.co"));
}

#[test]
fn fix_breaking_sweep_and_quote_copy() {
    let changes = include_str!("../CHANGELOG.md").split("## 0.10.1").next().unwrap();
    for term in ["**Breaking**", "below_gas_floor", "--self-submit", "--via market", "meta-aggregator", "taker"] {
        assert!(changes.contains(term), "missing {term}");
    }
    let readme = include_str!("../README.md");
    assert!(readme.contains("meta-aggregator") && readme.contains("--taker"));
}

#[test]
fn fix_quote_commands_accept_taker() {
    let taker = alloy::primitives::Address::repeat_byte(6).to_string();
    for tail in [vec!["quote", "--from", "USDC", "--to", "WETH"], vec!["batch-quote", "USDC/WETH"]] {
        let mut args = vec!["agentswap"];
        args.extend(tail);
        args.extend(["--chainid", "56", "--amount", "1000000", "--taker", &taker]);
        assert!(Cli::try_parse_from(args).is_ok());
    }
}

#[tokio::test]
async fn fix_missing_taker_refuses_before_token_reads() {
    let client = client::Client::new("http://127.0.0.1:1", None);
    let input = serde_json::from_value(serde_json::json!({"chain_id":"56", "from":"UNKNOWN", "to":"UNKNOWN", "amount":"1"})).unwrap();
    let error = service::quote::quote(&client, input).await.unwrap_err().to_string();
    assert!(error.contains("--taker") && error.contains("V6 proxy"), "{error}");
}

pub(crate) fn metadata_rpc() -> test_rpc::TestRpc {
    use alloy::{primitives::U256, sol_types::{SolCall, SolValue}};
    test_rpc::TestRpc::start(|body| {
        if body["method"] != "eth_call" { return Some(test_rpc::failure(body, "unexpected RPC method")); }
        let tx = &body["params"][0];
        let data = tx["input"].as_str().or(tx["data"].as_str()).unwrap();
        let data = hex::decode(data.trim_start_matches("0x")).unwrap();
        let bytes = if data.starts_with(&order_types::Erc20Metadata::decimalsCall::SELECTOR) {
            U256::from(18).abi_encode()
        } else { "TEST".to_string().abi_encode() };
        Some(test_rpc::ok(body, serde_json::json!(format!("0x{}", hex::encode(bytes)))))
    })
}

#[test]
fn fix_cli_quotes_forward_taker_using_central_meta_origin() {
    const NAME: &str = "fix_tests::fix_cli_quotes_forward_taker_using_central_meta_origin";
    if let Ok(url) = std::env::var("FIX_META_URL") {
        routes::TEST_META_ORIGIN.set(url).unwrap();
        routes::TEST_APP_ORIGIN.set("http://127.0.0.1:1".into()).unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let from = alloy::primitives::Address::repeat_byte(1).to_string();
            let to = alloy::primitives::Address::repeat_byte(2).to_string();
            let taker = alloy::primitives::Address::repeat_byte(6).to_string();
            let pair = format!("{from}/{to}");
            for tail in [vec!["quote", "--from", &from, "--to", &to], vec!["batch-quote", &pair]] {
                let mut args = vec!["agentswap", "--json", "--api-key", "synthetic-key"];
                args.extend(tail);
                args.extend(["--chainid", "56", "--amount", "1000000", "--taker", &taker]);
                run_cli(Cli::try_parse_from(args).unwrap()).await.unwrap();
            }
        }); return;
    }
    let rpc = metadata_rpc();
    let meta = http::Server::start(vec![(200, String::new(), client::meta_tests::best().to_string())]);
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
        .env("FIX_META_URL", &meta.url).env("AGENTSWAP_RPC_URL_56", &rpc.url).output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(rpc.called("eth_call") > 0);
    let seen = meta.requests.lock().unwrap();
    assert_eq!(seen.len(), 2);
    for request in seen.iter() {
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, "/quote");
        let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["taker"], alloy::primitives::Address::repeat_byte(6).to_string());
        assert_eq!(body["chainId"], 56);
        assert!(!request.keyed && !request.paid);
    }
}
