// Shared display helpers for AgentSwap CLI formatting.
// Exports: short_addr, format_duration, add_tx_status_rows, tx_url.
// Deps: comfy_table, crate::service::submit::TxStatus.

use crate::service::submit::TxStatus;
use comfy_table::Table;

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

/// The explorer page of a transaction, on chains whose explorer the chain table knows.
pub fn tx_url(chain_id: u64, hash: &str) -> Option<String> {
    crate::tokens::chain_id_to_explorer(chain_id).map(|explorer| format!("{explorer}/tx/{hash}"))
}

/// Add the receipt outcome of a broadcast transaction to a result table.
pub fn add_tx_status_rows(table: &mut Table, status: Option<TxStatus>, error: Option<&str>) {
    if let Some(status) = status {
        table.add_row(vec!["Tx Status", status.as_str()]);
    }
    if let Some(error) = error {
        table.add_row(vec!["Tx Error", error]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tx_url_exists_only_where_the_chain_table_has_an_explorer() {
        assert_eq!(tx_url(5042002, "0xab").as_deref(), Some("https://testnet.arcscan.app/tx/0xab"));
        assert_eq!(tx_url(8453, "0xab"), None);
    }

    #[test]
    fn tx_status_rows_show_the_status_and_why_it_is_unknown() {
        let mut table = Table::new();
        add_tx_status_rows(&mut table, Some(TxStatus::Unknown), Some("no receipt before the wait ended"));
        add_tx_status_rows(&mut table, None, None);
        let text = table.to_string();
        assert!(text.contains("Tx Status") && text.contains("unknown"), "{text}");
        assert!(text.contains("no receipt before the wait ended"), "{text}");
        assert_eq!(table.row_count(), 2);
    }

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
