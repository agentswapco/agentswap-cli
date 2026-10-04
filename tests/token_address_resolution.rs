// `quote` and `trade` accept a raw token address on every V6 chain, including chains without a
// built-in registry: an address outside the registry is read on chain before any quote request.
// Offline: the service URL and every RPC URL point at a refused local port, so a run that gets past
// token resolution fails on that transport instead. Control: registry symbols on Base.
use std::process::Command;

const REFUSED: &str = "http://127.0.0.1:9";

fn run(args: &[&str]) -> (i32, String) {
    let home = std::env::temp_dir().join(format!("agentswap-token-resolution-{}", std::process::id()));
    std::fs::create_dir_all(&home).expect("temp home");
    let key = home.join("agent.key");
    std::fs::write(&key, "01".repeat(32)).expect("key file");
    let out = Command::new(env!("CARGO_BIN_EXE_agentswap"))
        .args(["--url", REFUSED, "--key-file"])
        .arg(&key)
        .args(args)
        .env("HOME", &home)
        .env("AGENTSWAP_RPC_URL", REFUSED)
        .env_remove("SR_API_KEY")
        .env_remove("AGENTSWAP_URL")
        .env_remove("AGENTSWAP_KEY_FILE")
        .output()
        .expect("run agentswap");
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
}

const CASES: [(&str, &str, &str); 3] = [
    ("56", "0x55d398326f99059fF775485246999027B3197955", "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"),
    ("4663", "0x0000000000000000000000000000000000000001", "0x0000000000000000000000000000000000000002"),
    ("5042002", "0x3600000000000000000000000000000000000001", "0x0000000000000000000000000000000000000002"),
];

#[test]
fn control_registry_symbols_on_base_reach_the_transport() {
    let (code, stderr) = run(&["quote", "--chainid", "8453", "--from", "USDC", "--to", "WETH", "--amount", "1000000"]);
    assert_eq!(code, 1, "no service listens on {REFUSED}: {stderr}");
    assert!(!stderr.contains("unknown token"), "{stderr}");
}

#[test]
fn raw_addresses_on_v6_chains_without_a_registry_are_read_on_chain() {
    for (chain, from, to) in CASES {
        for command in ["quote", "trade"] {
            let mut args = vec![command, "--chainid", chain, "--from", from, "--to", to, "--amount", "1000000"];
            if command == "trade" {
                args.extend(["--proxy", "0x2222222222222222222222222222222222222222"]);
            }
            let (code, stderr) = run(&args);
            assert_eq!(code, 1, "{command} on {chain}: {stderr}");
            assert!(!stderr.contains("unknown token"), "{command} on {chain} refused the address: {stderr}");
            assert!(stderr.contains("does not answer decimals()"), "{command} on {chain}: {stderr}");
        }
    }
}

#[test]
fn a_raw_address_outside_the_registry_on_a_chain_without_v6_is_refused_before_any_request() {
    let (code, stderr) = run(&["quote", "--chainid", "1", "--from", CASES[0].1, "--to", "WETH", "--amount", "1"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("only on a chain with a V6 deployment"), "{stderr}");
}
