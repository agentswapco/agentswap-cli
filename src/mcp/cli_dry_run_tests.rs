// CLI executable regressions for unsigned dry-run JSON and human-readable previews.
// Exports: ignored E2E tests, run with AGENTSWAP_E2E_BIN on an authorized remote host.
// Deps: shared quote/RPC fixture, std process/filesystem, serde_json.

use super::dry_run_fixture::Fixture;
use alloy::primitives::Address;
use alloy::sol_types::SolCall;
use crate::order_types::{IntentSettlerV3, UserProxyV6};
use std::process::Command;

fn check_cli(is_intent: bool, requested: bool) {
    let fixture = Fixture::start();
    let directory = std::env::temp_dir().join(format!("agentswap-e2e-{}-{is_intent}-{requested}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let key = directory.join("test-key");
    std::fs::write(&key, "01".repeat(32)).unwrap();
    for json in [true, false] {
        let mut command = Command::new(std::env::var("AGENTSWAP_E2E_BIN").expect("remote CLI executable"));
        command.env_clear().env("AGENTSWAP_RPC_URL_8453", &fixture.url).current_dir(&directory);
        command.args(["--url", &fixture.url, "--api-key", "test", "--key-file"]).arg(&key);
        if json { command.arg("--json"); }
        if requested { command.arg("--allow-trade"); }
        if is_intent {
            command.args(["intent", "place", "--proxy-owner", &format!("{:?}", Address::repeat_byte(4)),
                "--start-out", "2", "--end-out", "1", "--relay"]);
        } else {
            command.args(["trade", "--proxy", &format!("{:?}", Address::repeat_byte(2)), "--self-submit"]);
        }
        command.args(["--chainid", "8453", "--from", "USDC", "--to", "WETH", "--amount", "1"]);
        if requested { command.arg("--dry-run"); }
        let output = command.output().expect("CLI process");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_preview(&String::from_utf8(output.stdout).unwrap(), json, is_intent);
    }
    std::fs::remove_dir_all(directory).unwrap();
    let calls = fixture.calls.lock().unwrap();
    let hash = if is_intent { UserProxyV6::hashIntentAuthorizationCall::SELECTOR } else { UserProxyV6::hashAgentOrderCall::SELECTOR };
    assert!(calls.contains(&hex::encode(hash)), "digest parity RPC missing");
    if is_intent { assert!(calls.contains(&hex::encode(IntentSettlerV3::orderHashCall::SELECTOR))); }
    assert!(!calls.contains(&crate::routes::INTENT_ANNOUNCE.to_string()));
    assert!(!calls.contains(&hex::encode(UserProxyV6::isIntentAuthorizedCall::SELECTOR)));
}

fn assert_preview(output: &str, json: bool, is_intent: bool) {
    if json {
        let value: serde_json::Value = serde_json::from_str(output).expect("CLI JSON");
        assert_eq!(value["dry_run"], true);
        for field in ["signature", "envelope", "self_submit"] {
            assert!(value.get(field).is_none(), "CLI dry-run released {field}");
        }
        assert!(value["order"].is_object());
        assert_eq!(value["digest"].as_str().unwrap().len(), 66);
        if is_intent { assert!(value["authorization"].is_object()); }
        else { assert_eq!(value["quote"]["output"], "1000"); }
        return;
    }
    assert!(output.contains("Digest"));
    assert!(output.contains("true"));
    for label in ["Signature", "Envelope", "Self Submit Calldata"] {
        assert!(!output.contains(label), "CLI dry-run displayed {label}");
    }
}

#[test]
#[ignore = "requires remote AGENTSWAP_E2E_BIN"]
fn requested_trade_cli_dry_run() { check_cli(false, true); }

#[test]
#[ignore = "requires remote AGENTSWAP_E2E_BIN"]
fn forced_trade_cli_dry_run() { check_cli(false, false); }

#[test]
#[ignore = "requires remote AGENTSWAP_E2E_BIN"]
fn requested_intent_cli_dry_run() { check_cli(true, true); }

#[test]
#[ignore = "requires remote AGENTSWAP_E2E_BIN"]
fn forced_intent_cli_dry_run() { check_cli(true, false); }
