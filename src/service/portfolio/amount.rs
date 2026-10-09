// Exact decimal parsing and rendering for portfolio values and advisory grant caps.
// Uses integer digit strings and wide products; no monetary value passes through a float.
use alloy::primitives::{U256, U512};
use eyre::{Result, eyre};

pub fn render(raw: &str, decimals: usize) -> String {
    let raw = raw.trim_start_matches('0');
    if raw.is_empty() { return "0".into(); }
    if decimals == 0 { return raw.into(); }
    let padded = format!("{:0>width$}", raw, width = decimals + 1);
    let split = padded.len() - decimals;
    format!("{}.{}", &padded[..split], &padded[split..]).trim_end_matches('0').trim_end_matches('.').into()
}

pub fn fixed(value: &str, exponent_allowed: bool) -> Result<U256> {
    let (mantissa, exponent) = match value.split_once(['e', 'E']) {
        Some((m, e)) if exponent_allowed => (m, e.parse::<i32>()?),
        Some(_) => return Err(eyre!("USD threshold must be an unsigned decimal")),
        None => (value, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty() || (mantissa.contains('.') && fraction.is_empty()) || !whole.bytes().chain(fraction.bytes()).all(|c| c.is_ascii_digit()) {
        return Err(eyre!("invalid unsigned decimal"));
    }
    let digits = format!("{whole}{fraction}");
    let shift = 18i64 + i64::from(exponent) - fraction.len() as i64;
    let scaled = if shift >= 0 {
        if shift > 77 { return Err(eyre!("decimal overflow")); }
        format!("{digits}{}", "0".repeat(shift as usize))
    } else {
        let keep = digits.len().saturating_sub((-shift) as usize);
        if !exponent_allowed && digits[keep..].bytes().any(|c| c != b'0') {
            return Err(eyre!("USD threshold supports up to 18 decimal places"));
        }
        digits[..keep].to_string()
    };
    if scaled.is_empty() { return Ok(U256::ZERO); }
    crate::order_types::parse_raw_amount("USD value", &scaled)
}

pub fn valuation(balance: U256, price: U256, decimals: u8, max: Option<U256>) -> (String, bool) {
    let product = U512::from(balance) * U512::from(price);
    let digits = product.to_string();
    let split = digits.len().saturating_sub(decimals as usize);
    let quotient = if split == 0 { "0" } else { &digits[..split] };
    let remainder = digits[split..].bytes().any(|c| c != b'0');
    let below = max.is_some_and(|max| {
        let limit = max.to_string();
        quotient.len() < limit.len() || (quotient.len() == limit.len()
            && (quotient < limit.as_str() || (quotient == limit && !remainder)))
    });
    (render(quotient, 18), below)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_decimals_and_boundaries() {
        assert_eq!(render("1234500", 6), "1.2345");
        assert_eq!(render("5", 18), "0.000000000000000005");
        assert_eq!(render("000", 18), "0");
        assert_eq!(render("1000000", 6), "1");
        assert_eq!(render("12", 0), "12");
        let price = fixed("1.000000000000000001", true).unwrap();
        assert!(!valuation(U256::from(1), price, 0, Some(fixed("1", false).unwrap())).1);
        assert!(valuation(U256::from(1), fixed("1", true).unwrap(), 0, Some(fixed("1", false).unwrap())).1);
        assert!(!valuation(U256::from(1), U256::from(1), 255, Some(U256::ZERO)).1);
        assert_eq!(fixed("1e-18", true).unwrap(), U256::from(1));
        for bad in ["-1", "NaN", "1e2", ".5", "1.0000000000000000001"] {
            assert!(fixed(bad, false).is_err(), "{bad}");
        }
    }
}
