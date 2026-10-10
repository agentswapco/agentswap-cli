// Real-fork batch-sale flow: plan, owner grant, receipts, proceeds, depletion and stale generation.
// Ignored by default; run with AGENTSWAP_FORK_URL set and `cargo test -- --ignored`.
// CHAIN=56 also requires BSC_WBNB, BSC_ETH and BSC_USDT.
#[path = "batch_sell_fork_fixture.rs"]
mod fixture;
#[path = "client/test_server.rs"]
mod http;
#[path = "service/test_rpc.rs"]
mod proxy_rpc;
use fixture::{Fork, rpc, ExecutedAsAgent};
use alloy::{primitives::{Address, U256}, sol_types::{SolCall, SolEvent}, providers::Provider};
use serde_json::{Value, json};
const NAME: &str = "batch_sell_fork_tests::batch_sell_fork_owner_confirmed_market";
const ID: &str = "abcdefghijklmnopqrstuv";

#[test]
#[ignore = "requires AGENTSWAP_FORK_URL"]
fn batch_sell_fork_owner_confirmed_market() {
    if let Ok(app) = std::env::var("BATCH_FORK_CHILD") {
        use clap::Parser;
        crate::routes::TEST_APP_ORIGIN.set(app).unwrap();
        let args: Vec<String> = serde_json::from_str(&std::env::var("BATCH_FORK_ARGS").unwrap()).unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(crate::run_cli(crate::cli::Cli::try_parse_from(args).unwrap())).unwrap();
        return;
    }
    let url = std::env::var("AGENTSWAP_FORK_URL").unwrap_or_else(|_| panic!("{NAME} needs AGENTSWAP_FORK_URL"));
    let fork = Fork::start(&url);
    tokio::runtime::Runtime::new().unwrap().block_on(flow(&fork));
}

async fn flow(fork: &Fork) {
    let proxy = fork.setup().await;
    let prices = prices(fork).await;
    let request = plan(fork, &prices).await;
    // Increase balances after planning: both request caps remain below current holdings.
    fork.send(fork.tokens[0], fixture::depositCall {}.abi_encode(),fork.cap).await;
    fork.balance_second(fork.cap*U256::from(2)).await;
    fork.grant(proxy).await;
    let record = confirmed(fork, request).await;
    assert_eq!(record["confirmed"]["proxy"],json!(proxy));
    let before = receive_balance(fork).await;
    let output = run(fork, &record, &prices, false);
    receipts(fork,proxy,&output,before).await;
    let second = run(fork,&record,&prices,false);
    let rows = second["tokens"].as_array().unwrap();
    assert_eq!(rows.len(),3);
    assert!(rows.iter().all(|r|r["outcome"]=="skipped" && matches!(r["reason"].as_str(),Some("zero"|"receive_token"))),"{second}");
    fork.grant(proxy).await;
    let fresh = confirmed(fork,record["request"].clone()).await;
    assert_ne!(fresh["confirmed"]["generation"],record["confirmed"]["generation"]);
    run(fork,&record,&prices,true);
}

async fn prices(fork: &Fork) -> Value {
    let addresses = fork.tokens.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
    let response: Value = reqwest::Client::new().get(format!("{}/api/prices",crate::routes::APP_ORIGIN))
        .query(&[("chainId",fork.chain.to_string()),("addresses",addresses)]).timeout(std::time::Duration::from_secs(30)).send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    assert_eq!(response["prices"].as_object().unwrap().len(),3,"fork requires prices for all tokens");
    response
}

async fn plan(fork: &Fork, prices: &Value) -> Value {
    let holdings: Vec<_> = fork.tokens[..2].iter().map(|a|json!({"address":a,"balanceRaw":fork.cap.to_string(),"decimals":18,"symbol":"FORK"})).collect();
    let app = http::Server::start(vec![
        (200,String::new(),json!({"chainId":fork.chain,"owner":fork.owner,"indexed":true,"truncated":false,"tokens":holdings}).to_string()),
        (200,String::new(),prices.to_string()),(200,String::new(),prices.to_string()),
        (201,String::new(),json!({"id":ID,"url":format!("https://app.agentswap.co/grant/r/{ID}"),"expiresAt":"2099-01-01T00:00:00Z"}).to_string())]);
    let mut args = vec!["agentswap".into(),"--json".into(),"batch-sell".into(),"plan".into(),"--chainid".into(),fork.chain.to_string(),
        "--owner".into(),fork.owner.to_string(),"--agent".into(),fork.agent.to_string(),"--receive".into(),fork.tokens[2].to_string(),"--max-loss-bps".into(),"500".into()];
    for token in &fork.tokens[..2] { args.extend(["--token".into(),token.to_string()]); }
    let output = fork.cli(&app.url,&fork.rpc,&args).unwrap();
    assert_eq!(output["count"],2,"{output}");
    assert_eq!(output["requests"].as_array().unwrap().len(),1);
    let requests = app.requests.lock().unwrap();
    let posts: Vec<_> = requests.iter().filter(|r|r.method=="POST").collect();
    assert_eq!(posts.len(),1); assert_eq!(posts[0].target,"/api/grant-requests");
    assert!(!posts[0].keyed && !posts[0].paid);
    let body: Value = serde_json::from_str(&posts[0].body).unwrap();
    assert_request(fork,&body);
    body
}

fn assert_request(fork: &Fork, body: &Value) {
    let rows = body["tokens"].as_array().unwrap();
    assert_eq!(rows.len(),3);
    let mut addresses: Vec<_> = rows[..2].iter().map(|t| {
        assert_eq!(t["cap"],crate::service::portfolio::amount::render(&fork.cap.to_string(),18)); t["address"].as_str().unwrap().parse::<Address>().unwrap()
    }).collect();
    addresses.sort();
    let mut expected = fork.tokens[..2].to_vec(); expected.sort();
    assert_eq!(addresses,expected);
    assert_eq!(rows[2]["address"].as_str().unwrap().parse::<Address>().unwrap(),fork.tokens[2]);
    assert_eq!(rows[2]["cap"],"0");
    assert_eq!(body["v"],1); assert_eq!(body["chainId"],fork.chain);
    assert_eq!(body["agent"].as_str().unwrap().parse::<Address>().unwrap(),fork.agent);
    assert_eq!(body["owner"].as_str().unwrap().parse::<Address>().unwrap(),fork.owner);
    assert_eq!(body["epoch"],"1w"); assert_eq!(body["actions"],json!(["market","intent"]));
    assert_eq!(body["purpose"],"batch-sell"); assert_eq!(body["maxLossBps"],500);
    for field in ["label","note","signature"] { assert!(body.get(field).unwrap().is_null()); }
    assert_eq!(body.as_object().unwrap().len(),13);
    let expiry = chrono::DateTime::parse_from_rfc3339(body["expiry"].as_str().unwrap()).unwrap().timestamp();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    assert!((86300..=86400).contains(&(expiry-now)));
}

async fn confirmed(fork: &Fork, request: Value) -> Value {
    let provider = crate::evm::read_provider(&fork.rpc).unwrap();
    let config = crate::evm::chain_config(&fork.chain.to_string()).unwrap();
    let proxy = crate::order_types::UserProxyFactoryV6::new(config.factory,provider.clone()).proxyOf(fork.owner).call().await.unwrap();
    let policy = crate::order_types::UserProxyV6::new(proxy,provider).policyOf(fork.agent).call().await.unwrap();
    json!({"id":ID,"status":"confirmed","request":request,
        "confirmed":{"proxy":proxy,"generation":policy.generation.to_string(),"maxLossBps":400}})
}

fn run(fork: &Fork, record: &Value, prices: &Value, changed: bool) -> Value {
    let app = http::Server::start(vec![(200,String::new(),record.to_string()),(200,String::new(),prices.to_string())]);
    let url = fork.rpc.clone();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let rpc_proxy = proxy_rpc::TestRpc::start(move |body| Some(runtime.block_on(async {
        reqwest::Client::new().post(&url).json(body).send().await.unwrap().json().await.unwrap()
    })));
    // Market mode always self-submits; no obsolete --self-submit override is exposed.
    let args = ["agentswap","--allow-trade","--json","batch-sell","run","--request",ID,"--via","market","--key-file",fork.key.to_str().unwrap()].map(String::from);
    let output = fork.cli(&app.url,&rpc_proxy.url,&args);
    assert_eq!(app.requests.lock().unwrap()[0].target,format!("/api/grant-requests/{ID}"));
    if changed {
        assert!(output.unwrap_err().contains("confirmed grant generation changed"));
        assert_eq!(rpc_proxy.called("eth_sendRawTransaction"),0);
        Value::Null
    } else {
        let output = output.unwrap();
        if output["tokens"].as_array().unwrap().iter().all(|r|r["outcome"]=="skipped") {
            assert_eq!(rpc_proxy.called("eth_sendRawTransaction"),0);
        }
        output
    }
}

async fn receive_balance(fork: &Fork) -> U256 {
    let provider = crate::evm::read_provider(&fork.rpc).unwrap();
    crate::service::portfolio::discovery::balance(&provider,fork.tokens[2],fork.owner).await.unwrap()
}

async fn receipts(fork: &Fork, proxy: Address, output: &Value, before: U256) {
    let provider = crate::evm::read_provider(&fork.rpc).unwrap();
    let rows: Vec<_> = output["tokens"].as_array().unwrap().iter().filter(|r|r["outcome"]=="sold").collect();
    assert_eq!(rows.len(),2,"{output}");
    let mut proceeds = U256::ZERO;
    for row in rows {
        assert_eq!(row["amount_raw"],fork.cap.to_string());
        assert_eq!(row["tx_status"],"confirmed");
        let receipt = provider.get_transaction_receipt(row["tx_hash"].as_str().unwrap().parse().unwrap()).await.unwrap().unwrap();
        let event = receipt.inner.logs().iter().filter(|l|l.address()==proxy)
            .filter_map(|l|ExecutedAsAgent::decode_log(&l.inner).ok())
            .find(|e|Some(e.tokenIn)==row["token"].as_str().and_then(|s|s.parse::<Address>().ok()) && e.tokenOut==fork.tokens[2]).expect("execution receipt event");
        assert_eq!(event.agent,fork.agent); assert_eq!(event.amountIn,fork.cap);
        assert!(event.amountOut>U256::ZERO);
        assert_eq!(row["received_raw"],event.amountOut.to_string());
        proceeds += event.amountOut;
        let input: Address = row["token"].as_str().unwrap().parse().unwrap();
        assert_eq!(crate::service::portfolio::discovery::balance(&provider,input,fork.owner).await.unwrap(),fork.cap);
    }
    let after = receive_balance(fork).await;
    assert_eq!(after.checked_sub(before),Some(proceeds),"owner receive balance must rise by the summed amountOut");
    assert_eq!(rpc(&fork.rpc,"eth_chainId",json!([])).await,format!("0x{:x}",fork.chain));
}
