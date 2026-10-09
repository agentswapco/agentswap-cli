// Token resolution and amount-format helpers for the AgentSwap CLI.
// Exports: CHAIN_ID_HELP, V6_CHAINS_NOTE, unknown_chain_id, resolve_token, address_to_symbol,
// format_amount, chain_name_to_id, chain_id_to_name, chain_id_to_short.
// Deps: tokens::registry for static token metadata.
mod registry;

/// The supported chains for portfolio, grant-link and sweep commands.
pub const HOLDINGS_CHAINS_NOTE: &str = "Portfolio, grant links and sweep support Base (8453), Arbitrum One (42161), BNB Smart Chain (56) and Robinhood Chain (4663).";

/// The one published description of the chain selector. Every help string, MCP tool description,
/// input-schema field and runtime error that names a chain refers to this instead of pasting it.
pub const CHAIN_ID_HELP: &str = "Chain ID such as 8453; known aliases such as base are also accepted.";

/// The chains carrying a V6 deployment, stated on every command that needs one. An alias that
/// resolves to a chain without one is accepted by the quote commands and refused by these.
pub const V6_CHAINS_NOTE: &str = "V6 intents, policy and trade are available on Base (8453), Arbitrum One (42161), BNB Smart Chain (56), Robinhood Chain (4663), Arc (5042) and Arc Testnet (5042002) only; an alias that resolves to another chain is refused by those commands.";

/// The one published error for a chain selector that named no chain.
pub fn unknown_chain_id(value: &str) -> String {
    format!("unknown chain id: {value}. {CHAIN_ID_HELP}")
}

/// Resolve a chain ID or known alias (case-insensitive) to a chain ID.
pub fn chain_name_to_id(chain_id_or_alias: &str) -> Option<u64> {
    match chain_id_or_alias.to_lowercase().as_str() {
        "base" => Some(8453),
        "arbitrum" | "arb" => Some(42161),
        "bsc" | "bnb" | "binance" => Some(56),
        "robinhood" | "robinhood-chain" | "rh" => Some(4663),
        "arc" | "arc-mainnet" => Some(5042),
        "arc-testnet" | "arct" => Some(5042002),
        "ethereum" | "eth" | "mainnet" => Some(1),
        "optimism" | "op" => Some(10),
        _ => chain_id_or_alias.parse::<u64>().ok(),
    }
}

/// Resolve chain_id to display name.
pub fn chain_id_to_name(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "Ethereum",
        10 => "Optimism",
        8453 => "Base",
        42161 => "Arbitrum One",
        56 => "BNB Smart Chain",
        4663 => "Robinhood Chain",
        5042 => "Arc",
        5042002 => "Arc Testnet",
        _ => "Unknown",
    }
}

/// Short chain label for tables.
pub fn chain_id_to_short(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "Eth",
        10 => "Op",
        8453 => "Base",
        42161 => "Arb",
        56 => "BSC",
        4663 => "Robinhood",
        5042 => "Arc",
        5042002 => "Arc Testnet",
        _ => "?",
    }
}

pub fn chain_id_to_explorer(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        5042 => Some("https://explorer.arc.io"),
        5042002 => Some("https://testnet.arcscan.app"),
        _ => None,
    }
}

/// Token entry with address, symbol, and decimals.
struct TokenInfo {
    chain_id: u64,
    address: &'static str,
    symbol: &'static str,
    decimals: u8,
}

pub fn registry_addresses(chain_id: u64) -> Vec<&'static str> {
    registry::iter().filter(|token| token.chain_id == chain_id).map(|token| token.address).collect()
}

/// Resolve a token symbol or address to (address, symbol, decimals).
/// If input looks like an address (starts with 0x), returns it as-is with unknown decimals.
pub fn resolve_token(input: &str, chain_id: u64) -> Option<(&'static str, &'static str, u8)> {
    if input.starts_with("0x") || input.starts_with("0X") {
        for t in registry::iter() {
            if t.chain_id == chain_id && t.address.eq_ignore_ascii_case(input) {
                return Some((t.address, t.symbol, t.decimals));
            }
        }
        return None;
    }
    let needle = input.to_uppercase();
    for t in registry::iter() {
        if t.chain_id == chain_id && t.symbol.to_uppercase() == needle {
            return Some((t.address, t.symbol, t.decimals));
        }
    }
    None
}

/// Resolve an address to its symbol. Returns the symbol if found, otherwise truncated address.
#[allow(dead_code)]
pub fn address_to_symbol(address: &str, chain_id: u64) -> String {
    for t in registry::iter() {
        if t.chain_id == chain_id && t.address.eq_ignore_ascii_case(address) {
            return t.symbol.to_string();
        }
    }
    if address.len() > 10 {
        format!("{}..{}", &address[..6], &address[address.len() - 4..])
    } else {
        address.to_string()
    }
}

/// Format a raw amount string with token decimals for display.
/// e.g. "1000000000" with 6 decimals -> "1,000.000000"
pub fn format_amount(raw: &str, decimals: u8) -> String {
    let raw = raw.trim();
    if raw.is_empty() || decimals == 0 {
        return add_commas(raw);
    }
    let d = decimals as usize;
    let len = raw.len();
    let (integer, fraction) = if len <= d {
        let zeros = "0".repeat(d - len);
        ("0".to_string(), format!("{zeros}{raw}"))
    } else {
        (raw[..len - d].to_string(), raw[len - d..].to_string())
    };
    let trimmed = fraction.trim_end_matches('0');
    let frac = if trimmed.len() < 2 {
        &fraction[..2.min(fraction.len())]
    } else {
        trimmed
    };
    format!("{}.{frac}", add_commas(&integer))
}

fn add_commas(s: &str) -> String {
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len <= 3 {
        return s.to_string();
    }
    let mut result = String::with_capacity(len + len / 3);
    let remainder = len % 3;
    if remainder > 0 {
        result.push_str(&s[..remainder]);
    }
    for (i, chunk) in s.as_bytes()[remainder..].chunks(3).enumerate() {
        if i > 0 || remainder > 0 {
            result.push(',');
        }
        result.push_str(std::str::from_utf8(chunk).unwrap_or(""));
    }
    result
}

#[cfg(test)]
mod tests;
