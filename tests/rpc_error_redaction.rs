// An RPC transport error never prints the RPC URL's path or query, where provider keys live, on
// stderr or in --json output. Offline: the keyed URL points at a refused local port and carries a
// canary instead of a key; each case first checks that the run did fail on that transport.
use std::process::{Command, Output};

const CANARY: &str = "CANARY_SECRET_PATH";
const QUERY_CANARY: &str = "CANARY_SECRET_QUERY";

fn run(args: &[&str]) -> Output {
    let home = std::env::temp_dir().join(format!("agentswap-rpc-redaction-{}", std::process::id()));
    std::fs::create_dir_all(&home).expect("temp home");
    Command::new(env!("CARGO_BIN_EXE_agentswap"))
        .args(["--url", "http://127.0.0.1:9"])
        .args(args)
        .env("HOME", &home)
        .env("AGENTSWAP_RPC_URL_8453", format!("https://127.0.0.1:9/{CANARY}?key={QUERY_CANARY}"))
        .env_remove("AGENTSWAP_RPC_URL")
        .env_remove("SR_API_KEY")
        .output()
        .expect("run agentswap")
}

fn assert_redacted(output: &Output) {
    let (stdout, stderr) = (String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    for text in [&stdout, &stderr] {
        assert!(!text.contains(CANARY) && !text.contains(QUERY_CANARY), "the keyed RPC URL reached output: {text}");
    }
}

#[test]
fn rpc_transport_error_on_stderr_keeps_only_the_rpc_host() {
    let id = format!("0x{}1", "0".repeat(63));
    let output = run(&["intent", "status", "--chainid", "8453", "--id", &id]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "the RPC port is refused, so the command must fail: {stderr}");
    assert!(stderr.contains("Error:") && stderr.contains("127.0.0.1:9"), "precondition: a transport error: {stderr}");
    assert_redacted(&output);
}

#[test]
fn token_lookup_errors_in_quote_and_batch_quote_json_keep_only_the_rpc_host() {
    let token = "0x5555555555555555555555555555555555555555";
    let quote = run(&["quote", "--chainid", "8453", "--from", token, "--to", "WETH", "--amount", "1"]);
    assert_eq!(quote.status.code(), Some(1), "{}", String::from_utf8_lossy(&quote.stderr));
    assert_redacted(&quote);
    let pair = format!("{token}/WETH");
    let batch = run(&["--json", "batch-quote", "--chainid", "8453", "--amount", "1", &pair]);
    let stdout = String::from_utf8_lossy(&batch.stdout);
    assert!(stdout.contains("does not answer decimals()"), "precondition: the pair failed on the RPC: {stdout}");
    assert_redacted(&batch);
}
