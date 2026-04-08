// Shared display helpers for AgentSwap CLI formatting.
// Exports: short_addr, format_duration.
// Deps: std.

/// Truncate an address for table display: "0x1234...abcd"
pub fn short_addr(addr: &str) -> String {
    if addr.len() > 10 {
        format!("{}...{}", &addr[..6], &addr[addr.len() - 4..])
    } else {
        addr.to_string()
    }
}

/// Format seconds as human-readable duration: "3h 25m"
pub fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_addr_truncates_long_addresses() {
        assert_eq!(short_addr("0x1234567890abcdef"), "0x1234...cdef");
    }

    #[test]
    fn short_addr_leaves_short_strings_alone() {
        assert_eq!(short_addr("0x12345"), "0x12345");
    }

    #[test]
    fn format_duration_cases() {
        let cases = [(0, "0m"), (65, "1m"), (3700, "1h 1m"), (86400, "24h 0m")];

        for (input, expected) in cases {
            assert_eq!(format_duration(input), expected);
        }
    }
}
