// Arc token entries for the built-in CLI registry.
// Exports: TOKENS.
// Deps: super::super::TokenInfo.

use super::super::TokenInfo;

pub(super) static TOKENS: &[TokenInfo] = &[
    TokenInfo {
        chain_id: 5042,
        address: "0x3600000000000000000000000000000000000000",
        symbol: "USDC",
        decimals: 6,
    },
    TokenInfo {
        chain_id: 5042,
        address: "0xbEf5f6d51CB62b58e6A8f77868681825C6fe21c1",
        symbol: "EURC",
        decimals: 6,
    },
];
