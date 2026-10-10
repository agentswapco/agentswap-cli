// Human report: a Markdown table an agent can forward to the owner, then the summary and next
// steps. Amounts are shortened for reading; `--json` carries the exact raw values.
use super::models::{ReportOutput, ReportRow};
use comfy_table::{Table, presets::ASCII_MARKDOWN};
use std::fmt::Write;

/// `value` cut to `places` fractional digits; a nonzero value that would read as 0 keeps its digits.
fn short(value: &str, places: usize) -> String {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let fraction = &fraction[..fraction.len().min(places)];
    let text = if fraction.trim_end_matches('0').is_empty() { whole.to_string() } else { format!("{whole}.{}", fraction.trim_end_matches('0')) };
    if text == "0" && value.bytes().any(|b| matches!(b, b'1'..=b'9')) { value.to_string() } else { text }
}

/// Dollars with exactly two cents digits, cut rather than rounded.
fn usd(value: Option<&str>) -> String {
    let Some(value) = value else { return "-".into() };
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    format!("${whole}.{:0<2}", &fraction[..fraction.len().min(2)])
}

fn last_fill(row: &ReportRow) -> String {
    let Some(fill) = row.fills.last() else { return "-".into() };
    let hash = fill.tx_hash.as_deref().map_or_else(|| "unrecorded".into(), crate::display::short_addr);
    let extra = if row.fills.len() > 1 { format!(" (+{})", row.fills.len() - 1) } else { String::new() };
    format!("{hash} {}{extra}", fill.filled_at.as_deref().unwrap_or(""))
}

fn table(report: &ReportOutput) -> Table {
    let mut table = Table::new();
    table.load_preset(ASCII_MARKDOWN).set_header(vec!["Token", "Status", "Sold / cap", &format!("Received ({})", report.receive.symbol),
        "Value", "Discount", "Last fill"]);
    for row in &report.tokens {
        table.add_row(vec![row.symbol.clone(), row.status.replace('_', " "), format!("{} / {}", short(&row.sold, 6), short(&row.cap, 6)),
            row.received.as_deref().map_or_else(|| "-".into(), |r| short(r, 6)), usd(row.value_usd.as_deref()),
            row.discount_pct.as_deref().map_or_else(|| "-".into(), |d| format!("{d}%")), last_fill(row)]);
    }
    table
}

pub fn text(report: &ReportOutput) -> String {
    let s = &report.summary;
    let mut out = format!("Batch sale {} on {} ({}): receive {}, discount floor {}%\n{}\n\n", report.request,
        crate::tokens::chain_id_to_name(report.chain_id), report.chain_id, report.receive.symbol,
        super::rows::pct(i128::from(report.max_loss_bps)), table(report));
    let _ = writeln!(out, "Sold {}/{} tokens: {} {} received ({}) for {} at independent prices.", s.tokens_sold, s.tokens_total,
        short(&s.received, 6), report.receive.symbol, usd(s.received_usd.as_deref()), usd(Some(&s.sold_value_usd)));
    if let (Some(average), Some(worst)) = (&s.average_discount_pct, &s.worst_discount_pct) {
        let _ = writeln!(out, "Discount vs market: average {average}%, worst {worst}% on {}.", s.worst_discount_token.as_deref().unwrap_or("-"));
    }
    let _ = writeln!(out, "Unsold value: {}. Grant {} {}.", usd(Some(&s.unsold_value_usd)),
        if report.grant.active { "expires" } else { "ended; expiry" }, report.grant.expires_at);
    let _ = writeln!(out, "Values use independent prices as of {}, not prices at fill time.", report.as_of);
    for (title, lines) in [("Next", &report.next), ("Warnings", &report.warnings)] {
        if lines.is_empty() { continue; }
        let _ = writeln!(out, "{title}:");
        for line in lines { let _ = writeln!(out, "- {line}"); }
    }
    out
}
