// Ethereum token entries for the built-in CLI registry.
// Exports: TOKENS.
// Deps: super::super::TokenInfo.

use super::super::TokenInfo;

pub(super) static TOKENS: &[TokenInfo] = &[
    TokenInfo {
        chain_id: 1,
        address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
        symbol: "WETH",
        decimals: 18,
    },
    TokenInfo {
        chain_id: 1,
        address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        symbol: "USDC",
        decimals: 6,
    },
    TokenInfo {
        chain_id: 1,
        address: "0xdAC17F958D2ee523a2206206994597C13D831ec7",
        symbol: "USDT",
        decimals: 6,
    },
    TokenInfo {
        chain_id: 1,
        address: "0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599",
        symbol: "WBTC",
        decimals: 8,
    },
    TokenInfo {
        chain_id: 1,
        address: "0x6B175474E89094C44Da98b954EedeAC495271d0F",
        symbol: "DAI",
        decimals: 18,
    },
    TokenInfo {
        chain_id: 1,
        address: "0xae7ab96520DE3A18E5e111B5EaAb095312D7fE84",
        symbol: "stETH",
        decimals: 18,
    },
    TokenInfo {
        chain_id: 1,
        address: "0x7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0",
        symbol: "wstETH",
        decimals: 18,
    },
    TokenInfo {
        chain_id: 1,
        address: "0x5E8422345238F34275888049021821E8E08CAa1f",
        symbol: "frxETH",
        decimals: 18,
    },
    TokenInfo {
        chain_id: 1,
        address: "0xac3E018457B222d93114458476f3E3416Abbe38F",
        symbol: "sfrxETH",
        decimals: 18,
    },
    TokenInfo {
        chain_id: 1,
        address: "0x35fA164735182de50811E8e2E824cFb9B6118ac2",
        symbol: "eETH",
        decimals: 18,
    },
    TokenInfo {
        chain_id: 1,
        address: "0xCd5fE23C85820F7B72D0926FC9b05b43E359b7ee",
        symbol: "weETH",
        decimals: 18,
    },
    TokenInfo {
        chain_id: 1,
        address: "0x83F20F44975D03b1b09e64809B757c47f942BEeA",
        symbol: "sDAI",
        decimals: 18,
    },
    TokenInfo {
        chain_id: 1,
        address: "0x4c9EDD5852cd905f086C759E8383e09bff1E68B3",
        symbol: "USDe",
        decimals: 18,
    },
    TokenInfo {
        chain_id: 1,
        address: "0x9D39A5DE30e57443BfF2A8307A4256c8797A3497",
        symbol: "sUSDe",
        decimals: 18,
    },
];
