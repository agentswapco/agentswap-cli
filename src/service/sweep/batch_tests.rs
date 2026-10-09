// Batch-sell execution through the public CLI against isolated request, relay and RPC fixtures.
// Confirms the owner's discount wins and relay floor refusals do not stop other tokens.
use super::intent_tests::rpc;
use crate::{cli::Cli, signer::Signer, service::token};
use alloy::primitives::Address;
use clap::Parser;
use serde_json::{Value, json};
#[path = "../../client/test_server.rs"]
mod http;

#[test]
fn batch_sell_run_uses_confirmed_discount_and_continues_relay_422() {
    const NAME: &str = "service::sweep::batch_tests::batch_sell_run_uses_confirmed_discount_and_continues_relay_422";
    if let Ok(url) = std::env::var("BATCH_RUN_APP") {
        crate::routes::TEST_APP_ORIGIN.set(url).unwrap();
        let key = std::env::temp_dir().join(format!("batch-key-{}", std::process::id()));
        std::fs::write(&key, "01".repeat(32)).unwrap();
        let waiting = std::env::var("BATCH_WAIT").unwrap_or_else(|_| "0".into());
        let cli = Cli::try_parse_from(["agentswap", "--allow-trade", "--json", "batch-sell", "run", "--request",
            "https://app.agentswap.co/grant/r/abcdefghijklmnopqrstuv", "--key-file", key.to_str().unwrap(), "--wait", &waiting]).unwrap();
        let result = tokio::runtime::Runtime::new().unwrap().block_on(crate::run_cli(cli));
        std::fs::remove_file(key).unwrap();
        result.unwrap(); return;
    }
    for code in ["floor_below_confirmed_discount", "unpriced_for_confirmed_discount", "filled"] {
        let receive = token::from_registry("WETH", 8453).unwrap();
        let signer = crate::signer::local::LocalKey::from_private_key(&"01".repeat(32)).unwrap();
        let request = json!({"id":"abcdefghijklmnopqrstuv","status":"confirmed",
            "confirmed":{"proxy":Address::repeat_byte(6),"generation":"1","maxLossBps":100},
            "request":{"v":1,"chainId":8453,"owner":Address::repeat_byte(4),"agent":signer.address(),"purpose":"batch-sell","maxLossBps":500,
            "tokens":[{"address":Address::repeat_byte(1),"cap":"1"},{"address":Address::repeat_byte(2),"cap":"1"},{"address":receive.address,"cap":"0"}]}});
        let prices = json!({"prices":{Address::repeat_byte(1).to_string():{"priceUsd":2,"source":"defillama"},
            Address::repeat_byte(2).to_string():{"priceUsd":1,"source":"defillama"},receive.address:{"priceUsd":4,"source":"defillama"}}});
        let app = http::Server::start(vec![(200,String::new(),request.to_string()), (200,String::new(),prices.to_string()),
            (if code == "filled" {200} else {422},String::new(),json!({"error":code}).to_string()), (200,String::new(),"{\"accepted\":true}".into())]);
        let rpc = rpc(5, if code == "filled" { "filled" } else { "open" });
        let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",NAME,"--nocapture"])
            .env("BATCH_WAIT", if code == "filled" {"1"} else {"0"}).env("BATCH_RUN_APP", &app.url).env("AGENTSWAP_RPC_URL_8453", &rpc.url).output().unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{text} {}", String::from_utf8_lossy(&output.stderr));
        if code == "filled" { assert!(text.contains("sold") && text.contains("\"received_raw\": \"297000000000000000\""), "{text}"); }
        else { assert!(text.contains(code) && text.contains("placed"), "{text}"); }
        let requests = app.requests.lock().unwrap();
        let posts: Vec<_> = requests.iter().filter(|r| r.target == "/api/intents").collect();
        assert_eq!(posts.len(), 2);
        for (post, floor) in posts.iter().zip(["297000000000000000", "148500000000000000"]) {
            let body: Value = serde_json::from_str(&post.body).unwrap();
            assert_eq!(body["announce"]["order"]["endAmountOut"], floor);
        }
        assert_eq!(rpc.called("eth_sendRawTransaction"),0);
    }
}
