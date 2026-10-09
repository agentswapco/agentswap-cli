// Regression tests for tokens behavior.
// Uses the parent module and existing fixtures.
use super::*;

    #[test]
    fn chain_name_to_id_variants() {
        assert_eq!(chain_name_to_id("base"), Some(8453));
        assert_eq!(chain_name_to_id("BASE"), Some(8453));
        assert_eq!(chain_name_to_id("arb"), Some(42161));
        assert_eq!(chain_name_to_id("Ethereum"), Some(1));
        assert_eq!(chain_name_to_id("mainnet"), Some(1));
        assert_eq!(chain_name_to_id("op"), Some(10));
        assert_eq!(chain_name_to_id("bsc"), Some(56));
        assert_eq!(chain_name_to_id("robinhood"), Some(4663));
        assert_eq!(chain_name_to_id("arc"), Some(5042));
        assert_eq!(chain_name_to_id("arc-mainnet"), Some(5042));
        assert_eq!(chain_name_to_id("10"), Some(10));
        assert_eq!(chain_name_to_id("999"), Some(999));
        assert_eq!(chain_name_to_id("unknown"), None);
    }

    #[test]
    fn arc_chain_tables_and_v6_help() {
        for alias in ["arc", "arc-mainnet", "5042", "ARC", "ARC-MAINNET"] {
            assert_eq!(chain_name_to_id(alias), Some(5042));
        }
        assert_eq!(chain_id_to_name(5042), "Arc");
        assert_eq!(chain_id_to_short(5042), "Arc");
        assert!(V6_CHAINS_NOTE.contains("Arc (5042)"));
        use clap::CommandFactory;
        let mut cli = crate::cli::Cli::command();
        for name in ["policy", "trade"] {
            let help = cli.find_subcommand_mut(name).unwrap().render_long_help().to_string();
            assert!(help.contains("Arc (5042)"), "{name}: {help}");
        }
        let intent = cli.find_subcommand_mut("intent").unwrap();
        for name in ["place", "list", "status"] {
            let help = intent.find_subcommand_mut(name).unwrap().render_long_help().to_string();
            assert!(help.contains("Arc (5042)"), "intent {name}: {help}");
        }
    }

    #[test]
    fn arc_testnet_chain_tables_and_v6_help() {
        for alias in ["arc-testnet", "arct", "5042002", "ARC-TESTNET", "ARCT"] {
            assert_eq!(chain_name_to_id(alias), Some(5042002));
        }
        assert_eq!(chain_id_to_name(5042002), "Arc Testnet");
        assert_eq!(chain_id_to_short(5042002), "Arc Testnet");
        assert!(V6_CHAINS_NOTE.contains("Arc Testnet (5042002)"));
        use clap::CommandFactory;
        let mut cli = crate::cli::Cli::command();
        for name in ["policy", "trade"] {
            let help = cli.find_subcommand_mut(name).unwrap().render_long_help().to_string();
            assert!(help.contains("Arc Testnet (5042002)"), "{name}: {help}");
        }
        let intent = cli.find_subcommand_mut("intent").unwrap();
        for name in ["place", "list", "status"] {
            let help = intent.find_subcommand_mut(name).unwrap().render_long_help().to_string();
            assert!(help.contains("Arc Testnet (5042002)"), "intent {name}: {help}");
        }
    }

    #[test]
    fn arc_explorer_table() {
        assert_eq!(chain_id_to_explorer(5042), Some("https://explorer.arc.io"));
        assert_eq!(chain_id_to_explorer(5042002), Some("https://testnet.arcscan.app"));
        for chain in [8453, 42161, 56, 4663, 999] {
            assert_eq!(chain_id_to_explorer(chain), None);
        }
    }

    #[test]
    fn arc_testnet_explorer_table() {
        assert_eq!(chain_id_to_explorer(5042002), Some("https://testnet.arcscan.app"));
        for chain in [8453, 42161, 56, 4663, 999] {
            assert_eq!(chain_id_to_explorer(chain), None);
        }
    }

    #[test]
    fn arc_token_resolution() {
        assert_eq!(resolve_token("USDC", 5042), Some(("0x3600000000000000000000000000000000000000", "USDC", 6)));
        assert_eq!(resolve_token("usdc", 5042), Some(("0x3600000000000000000000000000000000000000", "USDC", 6)));
        assert_eq!(resolve_token("EURC", 5042), Some(("0xbEf5f6d51CB62b58e6A8f77868681825C6fe21c1", "EURC", 6)));
        assert_eq!(resolve_token("eurc", 5042), Some(("0xbEf5f6d51CB62b58e6A8f77868681825C6fe21c1", "EURC", 6)));
        assert_eq!(resolve_token("0x3600000000000000000000000000000000000000", 5042), Some(("0x3600000000000000000000000000000000000000", "USDC", 6)));
        assert_eq!(resolve_token("0xbEf5f6d51CB62b58e6A8f77868681825C6fe21c1", 5042), Some(("0xbEf5f6d51CB62b58e6A8f77868681825C6fe21c1", "EURC", 6)));
        assert_eq!(address_to_symbol("0x3600000000000000000000000000000000000000", 5042), "USDC");
        assert_eq!(address_to_symbol("0xbEf5f6d51CB62b58e6A8f77868681825C6fe21c1", 5042), "EURC");
    }

    #[test]
    fn unknown_chain_id_reuses_the_published_chain_selector_sentence() {
        assert_eq!(
            unknown_chain_id("not-a-chain"),
            format!("unknown chain id: not-a-chain. {CHAIN_ID_HELP}")
        );
    }

    #[test]
    fn chain_id_to_name_and_short() {
        assert_eq!(chain_id_to_name(1), "Ethereum");
        assert_eq!(chain_id_to_name(10), "Optimism");
        assert_eq!(chain_id_to_name(8453), "Base");
        assert_eq!(chain_id_to_name(42161), "Arbitrum One");
        assert_eq!(chain_id_to_name(56), "BNB Smart Chain");
        assert_eq!(chain_id_to_name(4663), "Robinhood Chain");
        assert_eq!(chain_id_to_name(5042), "Arc");
        assert_eq!(chain_id_to_name(5042002), "Arc Testnet");
        assert_eq!(chain_id_to_name(999), "Unknown");

        assert_eq!(chain_id_to_short(1), "Eth");
        assert_eq!(chain_id_to_short(10), "Op");
        assert_eq!(chain_id_to_short(8453), "Base");
        assert_eq!(chain_id_to_short(42161), "Arb");
        assert_eq!(chain_id_to_short(56), "BSC");
        assert_eq!(chain_id_to_short(4663), "Robinhood");
        assert_eq!(chain_id_to_short(5042), "Arc");
        assert_eq!(chain_id_to_short(5042002), "Arc Testnet");
        assert_eq!(chain_id_to_short(999), "?");
    }

    #[test]
    fn resolve_token_by_symbol_and_address() {
        assert_eq!(
            resolve_token("WETH", 1),
            Some(("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", "WETH", 18))
        );
        assert_eq!(
            resolve_token("usdc", 8453),
            Some(("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", "USDC", 6))
        );
        assert_eq!(
            resolve_token("0x4200000000000000000000000000000000000006", 8453),
            Some(("0x4200000000000000000000000000000000000006", "WETH", 18))
        );
        assert_eq!(
            resolve_token("0x4200000000000000000000000000000000000006", 1),
            None
        );
        assert_eq!(resolve_token("NON", 1), None);
    }

    #[test]
    fn address_to_symbol_variants() {
        assert_eq!(
            address_to_symbol("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", 1),
            "WETH"
        );
        assert_eq!(address_to_symbol("0xWatIsThIs", 1), "0xWatI..ThIs");
        assert_eq!(address_to_symbol("0x1234", 1), "0x1234");
    }

    #[test]
    fn format_amount_cases() {
        assert_eq!(format_amount("1000000000", 6), "1,000.00");
        assert_eq!(format_amount("123", 6), "0.000123");
        assert_eq!(format_amount("1234567", 0), "1,234,567");
        assert_eq!(format_amount("", 6), "");
    }

#[test]
fn holdings_help_uses_four_chain_note_without_changing_v6_support() {
    use clap::CommandFactory;
    let mut cli = crate::cli::Cli::command();
    for name in ["portfolio", "grant-link"] {
        let help = cli.find_subcommand_mut(name).unwrap().render_long_help().to_string();
        assert!(help.contains(HOLDINGS_CHAINS_NOTE), "{name}: {help}");
        assert!(!help.contains("Arc (5042)"));
    }
    assert!(V6_CHAINS_NOTE.contains("Arc (5042)"));
}
