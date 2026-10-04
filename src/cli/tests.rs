// Parser tests for the chain selector flags of every subcommand.
// Exports: no production symbols.
// Deps: parent cli module, clap.

use super::*;

#[test]
fn chainid_is_the_only_chain_selector_flag() {
    let cli = Cli::try_parse_from([
        "agentswap", "quote", "--chainid", "8453", "--from", "USDC", "--to", "WETH",
        "--amount", "1",
    ])
    .expect("--chainid should parse");
    let Commands::Quote { chain_id, .. } = cli.command else {
        panic!("expected quote command");
    };
    assert_eq!(chain_id, "8453");
    assert!(Cli::try_parse_from([
        "agentswap", "quote", "--chain", "base", "--from", "USDC", "--to", "WETH",
        "--amount", "1",
    ])
    .is_err());
}

#[test]
fn no_subcommand_still_takes_the_old_chain_flag() {
    // One rename that is only half done would leave the old flag alive on a command nobody
    // exercises, so every subcommand that selects a chain is probed here.
    let cases: [&[&str]; 6] = [
        &["agentswap", "tokens", "--chain", "base"],
        &["agentswap", "pools", "--chain", "base", "--address", "0x1"],
        &["agentswap", "batch-quote", "--chain", "base", "--amount", "1", "USDC/WETH"],
        &["agentswap", "policy", "--chain", "base", "--owner", "0x1", "--agent", "0x2"],
        &["agentswap", "intent", "list", "--chain", "base", "--owner", "0x1"],
        &["agentswap", "quota-claim", "--chain", "base", "--tx-hash", "0x1"],
    ];
    for case in cases {
        assert!(
            Cli::try_parse_from(case).is_err(),
            "--chain still parses for {:?}",
            case[1]
        );
    }
}

#[test]
fn the_x402_payment_chain_is_selected_by_chainid() {
    let cli = Cli::try_parse_from([
        "agentswap", "--x402-chainid", "42161", "tokens", "--chainid", "8453",
    ])
    .expect("--x402-chainid should parse");
    assert_eq!(cli.x402_chain_id, 42161);
    assert!(Cli::try_parse_from([
        "agentswap", "--x402-chain", "42161", "tokens", "--chainid", "8453",
    ])
    .is_err());
}
