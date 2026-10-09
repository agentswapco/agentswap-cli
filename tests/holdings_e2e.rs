// Executable CLI and MCP read-only holding flows against a scripted RPC/quote server.
// No live network, key files or signing; native-unit strings cross the process boundary intact.
#[path = "../src/service/test_rpc.rs"]
mod test_rpc;
use alloy::{primitives::{Address, U256, keccak256}, sol_types::SolValue};
use serde_json::{Value, json};
use std::{io::{BufRead, BufReader, Write}, process::{Command, Stdio}};
use test_rpc::{TestRpc, ok, failure};

fn fixture() -> TestRpc {
    TestRpc::start(|body| {
        let token = Address::repeat_byte(1);
        let result = match body["method"].as_str().unwrap_or("") {
            "eth_blockNumber" => json!("0x10"),
            "eth_getLogs" => json!([{"address":format!("{token:?}"), "topics":[], "data":"0x", "blockNumber":"0x10", "transactionIndex":"0x0", "logIndex":"0x0", "blockHash":alloy::primitives::B256::ZERO, "transactionHash":alloy::primitives::B256::ZERO, "removed":false}]),
            "alchemy_getTokenBalances" => return Some(failure(body, "method not found")),
            "eth_call" => {
                let tx = &body["params"][0];
                let data = tx["input"].as_str().or(tx["data"].as_str()).unwrap();
                let data = hex::decode(data.trim_start_matches("0x")).unwrap();
                let selector = &data[..4];
                let encoded = if selector == &keccak256("balanceOf(address)")[..4] { U256::from(1234500).abi_encode() }
                else if selector == &keccak256("decimals()")[..4] { U256::from(6).abi_encode() }
                else if selector == &keccak256("symbol()")[..4] { "TEST".to_string().abi_encode() }
                else if selector == &keccak256("proxyOf(address)")[..4] { Address::ZERO.abi_encode() }
                else { panic!("unexpected RPC selector"); };
                json!(format!("0x{}", hex::encode(encoded)))
            }
            _ => return Some(json!({"output":"99"})),
        };
        Some(ok(body, result))
    })
}

fn command(rpc: &TestRpc) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agentswap"));
    command.env_clear().env("AGENTSWAP_RPC_URL_4663", &rpc.url);
    command.args(["--url", &rpc.url, "--api-key", "fixture"]);
    command
}

#[test]
fn cli_portfolio_and_grant_link_ignore_signer_files() {
    let rpc = fixture();
    for json in [true, false] {
        for grant in [true, false] {
            let mut command = command(&rpc);
            command.args(["--key-file", "/nonexistent-readonly-test-key", "--x402"]);
            if json { command.arg("--json"); }
            command.arg(if grant { "grant-link" } else { "portfolio" });
            command.args(["--chainid", "4663", "--owner", &Address::repeat_byte(4).to_string(), "--token", &Address::repeat_byte(1).to_string()]);
            if grant { command.args(["--agent", &Address::repeat_byte(2).to_string(), "--receive", &Address::repeat_byte(3).to_string(), "--one-shot", "--note", "a+b & c"]); }
            else { command.args(["--max-usd", "10", "--quote-token", &Address::repeat_byte(3).to_string()]); }
            let output = command.output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            let text = String::from_utf8(output.stdout).unwrap();
            if json {
                let value: Value = serde_json::from_str(&text).unwrap();
                if grant {
                    assert_eq!(value["tokens"][0]["human"], "1.2345");
                    assert!(value["url"].as_str().unwrap().contains("note=a%2Bb+%26+c"));
                    assert!(value["replaced_policy"].is_null());
                } else {
                    assert_eq!(value["tokens"][0]["balance_raw"], "1234500");
                    assert_eq!(value["tokens"][0]["quote_out_raw"], "99");
                    assert_eq!(value["tokens"][0]["dust"], false);
                    assert!(value["indexer"].is_null());
                }
            } else { assert!(text.contains("1234500")); }
        }
    }
    assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
}

fn exchange(stdin: &mut impl Write, stdout: &mut impl BufRead, request: Value) -> Value {
    writeln!(stdin, "{request}").unwrap(); stdin.flush().unwrap();
    let mut line = String::new();
    assert!(stdout.read_line(&mut line).unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}

#[test]
fn mcp_portfolio_and_grant_link_over_stdio() {
    let rpc = fixture();
    let mut child = command(&rpc).arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let init = exchange(&mut stdin, &mut stdout, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}));
    assert!(init["result"]["instructions"].as_str().unwrap().contains("grant_link"));
    writeln!(stdin, "{}", json!({"jsonrpc":"2.0","method":"notifications/initialized"})).unwrap();
    let list = exchange(&mut stdin, &mut stdout, json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 11);
    for (id, tool) in [(3, "portfolio"), (4, "grant_link")] {
        let mut arguments = json!({"chain_id":"4663","owner":Address::repeat_byte(4).to_string(),"tokens":[Address::repeat_byte(1).to_string()]});
        if tool == "portfolio" { arguments["max_usd"] = json!("10"); arguments["quote_token"] = json!(Address::repeat_byte(3).to_string()); }
        else { arguments["agent"] = json!(Address::repeat_byte(2).to_string()); arguments["receive"] = json!(Address::repeat_byte(3).to_string()); arguments["one_shot"] = json!(true); }
        let response = exchange(&mut stdin, &mut stdout, json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":tool,"arguments":arguments}}));
        assert!(response.get("error").is_none(), "{response}");
        assert_ne!(response["result"]["isError"], true, "{response}");
        assert!(response.to_string().contains("1234500"), "{response}");
    }
    drop(stdin);
    assert!(child.wait().unwrap().success());
    assert_eq!(rpc.called("eth_sendRawTransaction"), 0);
}
