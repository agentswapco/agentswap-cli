// Executable batch-sell surface regression for help and argument requirements.
// Help and required-argument checks do not open RPC connections or load a signer.
use std::process::Command;

#[test]
fn batch_sell_executable_surface() {
    let binary = env!("CARGO_BIN_EXE_agentswap");
    for (sub, flags) in [("plan", vec!["--min-usd", "--max-usd", "--max-loss-bps", "--receive", "--owner", "--agent", "--token", "--exclude"]),
        ("run", vec!["--request", "--via", "--wait", "--key-file"])] {
        let help = Command::new(binary).args(["batch-sell", sub, "--help"]).output().unwrap();
        assert!(help.status.success());
        let help = String::from_utf8(help.stdout).unwrap();
        for flag in flags { assert!(help.contains(flag), "missing {flag}"); }
    }
    let removed = Command::new(binary).args(["sweep", "--help"]).output().unwrap();
    assert_eq!(removed.status.code(), Some(2));
    for name in ["portfolio", "grant-link"] {
        assert!(Command::new(binary).args([name, "--help"]).output().unwrap().status.success());
    }
}
