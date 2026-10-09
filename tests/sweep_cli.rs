// Executable sweep surface regression, runnable unchanged against the pre-feature commit.
// Help and required-argument checks do not open RPC connections or load a signer.
use std::process::Command;

#[test]
fn sweep_command_and_required_risk_inputs_are_exposed() {
    let binary = env!("CARGO_BIN_EXE_agentswap");
    let help = Command::new(binary).args(["sweep", "--help"]).output().unwrap();
    assert!(help.status.success(), "{}", String::from_utf8_lossy(&help.stderr));
    let help = String::from_utf8(help.stdout).unwrap();
    for flag in ["--max-usd", "--max-loss-bps", "--receive", "--proxy", "--self-submit", "--dry-run", "--token"] {
        assert!(help.contains(flag), "missing {flag}");
    }
    assert!(!help.contains("--lookback-blocks"));
    let portfolio = Command::new(binary).args(["portfolio", "--help"]).output().unwrap();
    assert!(portfolio.status.success());
    assert!(!String::from_utf8(portfolio.stdout).unwrap().contains("--lookback-blocks"));
    for absent in ["--max-usd", "--max-loss-bps", "--token"] {
        let mut args = vec!["sweep", "--chainid", "8453", "--proxy", "proxy", "--receive", "USDC"];
        if absent != "--token" { args.extend(["--token", "token"]); }
        if absent != "--max-usd" { args.extend(["--max-usd", "5"]); }
        if absent != "--max-loss-bps" { args.extend(["--max-loss-bps", "100"]); }
        let output = Command::new(binary).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains(absent));
    }
}

#[test]
fn sweep_help_states_key_loss_bound_and_unpriced_chains() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentswap")).args(["sweep", "--help"]).output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("Requires --key-file"), "{help}");
    assert!(help.contains("less than 10000"), "{help}");
    assert!(help.contains("sells nothing on chains without prices, including Robinhood Chain (4663)"), "{help}");
}

#[test]
fn gasless_sweep_cli_mode_and_wait() {
    let binary = env!("CARGO_BIN_EXE_agentswap");
    let output = Command::new(binary).args(["sweep", "--help"]).output().unwrap();
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--via") && help.contains("[default: intent]") && help.contains("--wait"), "{help}");
    for mode in ["intent", "market"] {
        let output = Command::new(binary).args(["sweep", "--chainid", "8453", "--proxy", "proxy", "--receive", "USDC",
            "--token", "token", "--max-usd", "5", "--max-loss-bps", "100", "--via", mode, "--wait", "0"]).output().unwrap();
        assert_ne!(output.status.code(), Some(2), "{}", String::from_utf8_lossy(&output.stderr));
    }
}
