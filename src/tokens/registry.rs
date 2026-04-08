// Static token registry composition for built-in symbol and address resolution.
// Exports: iter().
// Deps: chain-specific registry modules.

mod arbitrum;
mod base;
mod ethereum;

use super::TokenInfo;

pub(super) fn iter() -> impl Iterator<Item = &'static TokenInfo> {
    ethereum::TOKENS
        .iter()
        .chain(base::TOKENS.iter())
        .chain(arbitrum::TOKENS.iter())
}
