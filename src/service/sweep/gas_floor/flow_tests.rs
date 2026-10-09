// Intent gas admission and placement-bounded log regressions using loopback RPCs.
// Runs entrypoint-dependent reads in child processes to isolate chain environment variables.
use super::*;

#[test]
fn fix_intent_discounted_value_covers_fill_gas_or_warns() {
    const NAME: &str = "service::sweep::intent_tests::fix_tests::fix_intent_discounted_value_covers_fill_gas_or_warns";
    if let Ok(url) = std::env::var("FIX_INTENT_RPC") {
        tokio::runtime::Runtime::new().unwrap().block_on(gas_case(&url)); return;
    }
    for scenario in ["gas_high", "gas_missing", "native_missing", "gas_low"] {
        let rpc = rpc(5, match scenario { "gas_high" => "gas_high", "gas_missing" => "gas_missing", _ => "open" });
        let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
            .env("FIX_INTENT_RPC", &rpc.url).env("FIX_GAS_CASE", scenario).env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
        assert!(output.status.success(), "{scenario}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
    }
}

async fn gas_case(url: &str) {
    let scenario = std::env::var("FIX_GAS_CASE").unwrap();
    let receive = token::from_registry("USDC", 8453).unwrap();
    let native = token::from_registry("WETH", 8453).unwrap();
    let mut coins = json!({
        Address::repeat_byte(1).to_string():{"priceUsd":2,"source":"defillama"},
        Address::repeat_byte(2).to_string():{"priceUsd":1,"source":"defillama"},
        receive.address.clone():{"priceUsd":1,"source":"defillama"}
    });
    if scenario != "native_missing" { coins[&native.address] = json!({"priceUsd":4000,"source":"defillama"}); }
    let app = relay_server::Server::start(vec![(200, String::new(), json!({"prices":coins}).to_string())]);
    let relay = relay_server::Server::start(vec![(200, String::new(), "{}".into())]);
    let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
    let mut request = input(None, scenario != "gas_high"); request.receive = receive.address.clone();
    let out = read(&Client::new(&relay.url, None), signer, request, evm::read_provider(url).unwrap(), &app.url, None, Wait::MCP).await.unwrap();
    for row in out.tokens.iter().filter(|r| r.reason.as_deref() != Some("receive_token")) {
        let value = serde_json::to_value(row).unwrap();
        if scenario == "gas_high" { assert_eq!(row.reason.as_deref(), Some("below_gas_floor")); }
        else {
            assert_eq!(row.reason.as_deref(), Some("dry_run"));
            let warnings = value["warnings"].as_array().expect("row warnings");
            assert_eq!(warnings.is_empty(), scenario == "gas_low");
        }
    }
    assert!(relay.requests.lock().unwrap().is_empty());
    assert!(app.requests.lock().unwrap()[0].target.to_lowercase().contains(&native.address.to_lowercase()), "request wrapped native even outside basket");
}

#[test]
fn fix_wait_scans_only_blocks_since_placement() {
    const NAME: &str = "service::sweep::intent_tests::gasless_sweep_wait_reports_terminal_and_timeout_states";
    let rpc = rpc(5, "bounded");
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", NAME, "--nocapture"])
        .env("SWEEP_INTENT_RPC", &rpc.url).env("AGENTSWAP_RPC_URL_8453", &rpc.url).env("SWEEP_WAIT_STATUS", "open").output().unwrap();
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(rpc.called("eth_getLogs") > 0);
}
