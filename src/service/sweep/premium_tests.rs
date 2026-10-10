// Intent start-premium flows: the curve opens above market value and closes at the unchanged floor.
// JSON inputs keep these regressions executable against the revision without the premium.
use super::*;

fn premium_input(receive: &str, premium: Option<u16>, dry: bool) -> Input {
    let mut value = json!({"chain_id":"8453", "proxy":Address::repeat_byte(6), "receive":receive,
        "tokens":[Address::repeat_byte(1), Address::repeat_byte(2)], "max_usd":"5", "max_loss_bps":100,
        "dry_run":dry, "wait":0});
    if let Some(premium) = premium { value["start_premium_bps"] = json!(premium); }
    serde_json::from_value(value).unwrap()
}

fn child(name: &str, rpc: &TestRpc, scenario: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", name, "--nocapture"])
        .env("PREMIUM_RPC", &rpc.url).env("PREMIUM_CASE", scenario).env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
    assert!(output.status.success(), "{scenario}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
}

fn sold(output: &Output) -> Vec<&Row> {
    output.tokens.iter().filter(|r| r.reason.as_deref() != Some("receive_token")).collect()
}

#[test]
fn start_premium_moves_only_the_opening_price() {
    const NAME: &str = "service::sweep::intent_tests::premium_tests::start_premium_moves_only_the_opening_price";
    if let Ok(url) = std::env::var("PREMIUM_RPC") {
        tokio::runtime::Runtime::new().unwrap().block_on(curve_case(&url)); return;
    }
    child(NAME, &rpc(5, "open"), "curve");
}

// 0.6 tokens at 1.37 and 1 USD against WETH at 7 USD: market values 117428571428571428.57… and
// 85714285714285714.28… wei; every amount is the exact product rounded down once.
async fn curve_case(url: &str) {
    let receive = token::from_registry("WETH", 8453).unwrap();
    let prices = format!(r#"{{"prices":{{"{}":{{"priceUsd":1.37,"source":"defillama"}},"{}":{{"priceUsd":1,"source":"defillama"}},"{}":{{"priceUsd":7,"source":"defillama"}}}}}}"#,
        Address::repeat_byte(1), Address::repeat_byte(2), receive.address);
    let floors = ["116254285714285714", "84857142857142857"];
    for (premium, starts) in [(None, ["118602857142857142", "86571428571428571"]),
        (Some(0), ["117428571428571428", "85714285714285714"]), (Some(1000), ["129171428571428571", "94285714285714285"])] {
        let app = relay_server::Server::start(vec![(200, String::new(), prices.clone())]);
        let relay = relay_server::Server::start(vec![(200, String::new(), json!({"accepted":true}).to_string())]);
        let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
        let output = read(&Client::new(&relay.url, None), signer, premium_input(&receive.address, premium, false),
            evm::read_provider(url).unwrap(), &app.url, None, Wait::MCP).await.unwrap();
        output.check().unwrap();
        let requests = relay.requests.lock().unwrap();
        assert_eq!(requests.len(), 2, "{premium:?}");
        for (i, (row, request)) in sold(&output).into_iter().zip(requests.iter()).enumerate() {
            assert_eq!(row.start_out_raw.as_deref(), Some(starts[i]), "{premium:?}");
            assert_eq!(row.floor_raw.as_deref(), Some(floors[i]), "{premium:?}");
            let body: Value = serde_json::from_str(&request.body).unwrap();
            let order = &body["announce"]["order"];
            assert_eq!(order["startAmountOut"], starts[i]);
            assert_eq!(order["endAmountOut"], floors[i]);
            let time = |key: &str| order[key].as_str().unwrap().parse::<u64>().unwrap();
            assert_eq!(time("endTime") - time("startTime"), 600);
            assert_eq!(order["decayEndTime"], order["endTime"]);
        }
    }
}

#[test]
fn gas_floor_keeps_market_budget_under_start_premium() {
    const NAME: &str = "service::sweep::intent_tests::premium_tests::gas_floor_keeps_market_budget_under_start_premium";
    if let Ok(url) = std::env::var("PREMIUM_RPC") {
        tokio::runtime::Runtime::new().unwrap().block_on(gas_case(&url)); return;
    }
    for scenario in ["gas_border", "open"] { child(NAME, &rpc(5, scenario), scenario); }
}

// USDC receive at 1 USD, WETH at 2000 USD. Token 1 is worth 1200000 USDC units with a 1188000 floor.
// At 15000000 wei a Base fill costs about 16470 units: more than the 12000-unit discount budget,
// less than the 24000 units a 100 bps start would add. Token 2 has half the value.
async fn gas_case(url: &str) {
    let border = std::env::var("PREMIUM_CASE").unwrap() == "gas_border";
    let receive = token::from_registry("USDC", 8453).unwrap();
    let native = token::from_registry("WETH", 8453).unwrap();
    let prices = json!({"prices":{Address::repeat_byte(1).to_string():{"priceUsd":2,"source":"defillama"},
        Address::repeat_byte(2).to_string():{"priceUsd":1,"source":"defillama"},
        receive.address.clone():{"priceUsd":1,"source":"defillama"}, native.address.clone():{"priceUsd":2000,"source":"defillama"}}});
    for (premium, starts) in [(0, ["1200000", "600000"]), (100, ["1212000", "606000"]), (1000, ["1320000", "660000"])] {
        let app = relay_server::Server::start(vec![(200, String::new(), prices.to_string())]);
        let relay = relay_server::Server::start(vec![(200, String::new(), "{}".into())]);
        let signer = Arc::new(crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap());
        let output = read(&Client::new(&relay.url, None), signer, premium_input(&receive.address, Some(premium), true),
            evm::read_provider(url).unwrap(), &app.url, None, Wait::MCP).await.unwrap();
        for (i, row) in sold(&output).into_iter().enumerate() {
            assert_eq!(row.reason.as_deref(), Some(if border { "below_gas_floor" } else { "dry_run" }), "premium {premium}");
            assert_eq!(row.start_out_raw.as_deref(), Some(starts[i]), "premium {premium}");
            assert!(row.warnings.is_empty(), "premium {premium}: {:?}", row.warnings);
        }
        assert!(relay.requests.lock().unwrap().is_empty());
    }
}
