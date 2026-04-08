// Token resolution and amount-format helpers for the AgentSwap CLI.
// Exports: resolve_token, address_to_symbol, format_amount, scale_amount, chain_name_to_id.
// Deps: tokens::registry for static token metadata.
mod registry;

/// Resolve chain name (case-insensitive) to chain_id.
pub fn chain_name_to_id(name: &str) -> Option<u64> {
    match name.to_lowercase().as_str() {
        "base" => Some(8453),
        "arbitrum" | "arb" => Some(42161),
        "ethereum" | "eth" | "mainnet" => Some(1),
        "optimism" | "op" => Some(10),
        _ => name.parse::<u64>().ok(),
    }
}

/// Resolve chain_id to display name.
pub fn chain_id_to_name(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "Ethereum",
        10 => "Optimism",
        8453 => "Base",
        42161 => "Arbitrum",
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
        _ => "?",
    }
}

/// Token entry with address, symbol, and decimals.
struct TokenInfo {
    chain_id: u64,
    address: &'static str,
    symbol: &'static str,
    decimals: u8,
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

/// Scale a human-readable amount to raw units. "1000" with 6 decimals -> "1000000000".
pub fn scale_amount(human: &str, decimals: u8) -> String {
    if human.contains('.') {
        let parts: Vec<&str> = human.splitn(2, '.').collect();
        let integer = parts[0];
        let frac = parts.get(1).unwrap_or(&"");
        let d = decimals as usize;
        let padded = if frac.len() >= d {
            frac[..d].to_string()
        } else {
            format!("{frac}{}", "0".repeat(d - frac.len()))
        };
        let int_part = if integer.is_empty() { "0" } else { integer };
        let raw = format!("{int_part}{padded}");
        raw.trim_start_matches('0').to_string().max("0".to_string())
    } else {
        format!("{human}{}", "0".repeat(decimals as usize))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_name_to_id_variants() {
        assert_eq!(chain_name_to_id("base"), Some(8453));
        assert_eq!(chain_name_to_id("BASE"), Some(8453));
        assert_eq!(chain_name_to_id("arb"), Some(42161));
        assert_eq!(chain_name_to_id("Ethereum"), Some(1));
        assert_eq!(chain_name_to_id("mainnet"), Some(1));
        assert_eq!(chain_name_to_id("op"), Some(10));
        assert_eq!(chain_name_to_id("10"), Some(10));
        assert_eq!(chain_name_to_id("999"), Some(999));
        assert_eq!(chain_name_to_id("unknown"), None);
    }

    #[test]
    fn chain_id_to_name_and_short() {
        assert_eq!(chain_id_to_name(1), "Ethereum");
        assert_eq!(chain_id_to_name(10), "Optimism");
        assert_eq!(chain_id_to_name(8453), "Base");
        assert_eq!(chain_id_to_name(42161), "Arbitrum");
        assert_eq!(chain_id_to_name(999), "Unknown");

        assert_eq!(chain_id_to_short(1), "Eth");
        assert_eq!(chain_id_to_short(10), "Op");
        assert_eq!(chain_id_to_short(8453), "Base");
        assert_eq!(chain_id_to_short(42161), "Arb");
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
    fn scale_amount_cases() {
        assert_eq!(scale_amount("1000", 6), "1000000000");
        assert_eq!(scale_amount("1.5", 6), "1500000");
        assert_eq!(scale_amount("0.123", 6), "123000");
        assert_eq!(scale_amount("0.123456789", 6), "123456");
    }
}
