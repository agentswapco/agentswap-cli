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
    for absent in ["--max-usd", "--max-loss-bps"] {
        let mut args = vec!["sweep", "--chainid", "8453", "--proxy", "proxy", "--receive", "USDC"];
        if absent != "--max-usd" { args.extend(["--max-usd", "5"]); }
        if absent != "--max-loss-bps" { args.extend(["--max-loss-bps", "100"]); }
        let output = Command::new(binary).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains(absent));
    }
}
