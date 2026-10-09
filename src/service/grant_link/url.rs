// Grant-request v1 parameter ordering, cap validation and UTC expiry formatting.
// Uses WHATWG-compatible form encoding and exact integer caps.
use super::{Input, TokenSummary};
use crate::{order_types, service::portfolio::discovery};
use alloy::primitives::Address;
use eyre::{Result, eyre};
use chrono::{DateTime, SecondsFormat, Utc};
use std::collections::BTreeSet;

pub(super) fn schedule(input: &Input, now: u64) -> Result<(String, String)> {
    if input.one_shot {
        if input.epoch.is_some() || input.expiry.is_some() { return Err(eyre!("one-shot conflicts with epoch and expiry")); }
        let timestamp = i64::try_from(now.checked_add(86400).ok_or_else(|| eyre!("expiry overflow"))?)?;
        let expiry = DateTime::<Utc>::from_timestamp(timestamp, 0).ok_or_else(|| eyre!("invalid expiry"))?;
        return Ok(("1w".into(), expiry.to_rfc3339_opts(SecondsFormat::Secs, true)));
    }
    let epoch = input.epoch.as_deref().ok_or_else(|| eyre!("recurring grants require epoch"))?;
    if !matches!(epoch, "1h" | "1d" | "1w") { return Err(eyre!("epoch must be 1h, 1d or 1w")); }
    let expiry = input.expiry.as_deref().ok_or_else(|| eyre!("recurring grants require expiry"))?;
    if !matches!(expiry, "7d" | "30d" | "90d") {
        let date = DateTime::parse_from_rfc3339(expiry)?;
        if !expiry.ends_with('Z') || date.timestamp_subsec_nanos() != 0 || date.timestamp() <= i64::try_from(now)? {
            return Err(eyre!("expiry must be future UTC whole seconds ending Z"));
        }
        return Ok((epoch.into(), date.to_rfc3339_opts(SecondsFormat::Secs, true)));
    }
    Ok((epoch.into(), expiry.into()))
}

pub(super) fn requests(input: &Input, receive: &str) -> Result<Vec<(Address, Option<String>)>> {
    if input.tokens.is_empty() || input.tokens.len() >= 20 { return Err(eyre!("provide 1 to 19 spend tokens plus one receive token")); }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for entry in &input.tokens {
        let (address, cap) = entry.split_once(':').map_or((entry.as_str(), None), |(a, c)| (a, Some(c)));
        let address = discovery::erc20(address)?;
        if !seen.insert(address) { return Err(eyre!("duplicate spend token")); }
        let cap = cap.map(|s| order_types::parse_raw_amount("cap", s).map(|c| c.to_string())).transpose()?;
        if cap.is_none() && !input.one_shot { return Err(eyre!("recurring grants require explicit raw caps")); }
        out.push((address, cap));
    }
    let receive = discovery::erc20(receive)?;
    if !seen.insert(receive) { return Err(eyre!("receive token duplicates a spend token")); }
    out.push((receive, Some("0".into())));
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build(chain: u64, agent: &str, owner: Option<&str>, label: Option<&str>, note: Option<&str>, tokens: &[TokenSummary], epoch: &str, expiry: &str) -> Result<String> {
    let mut url = reqwest::Url::parse("https://app.agentswap.co/grant")?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("v", "1").append_pair("chain", &chain.to_string()).append_pair("agent", agent);
        if let Some(label) = label { query.append_pair("label", label); }
        for token in tokens { query.append_pair("t", &format!("{}:{}", token.address, token.human)); }
        query.append_pair("epoch", epoch).append_pair("expiry", expiry).append_pair("actions", "market");
        if let Some(note) = note { query.append_pair("note", note); }
        if let Some(owner) = owner { query.append_pair("owner", owner); }
    }
    Ok(url.into())
}
