// Sweep relayed placements and bounded status reads reuse the existing intent service.
// Price math and spend limits are checked by sale before reaching this signing boundary.
use super::{Context, Input, Output, Row};
use crate::service::intent::{self, PlaceInput, TokenPolicy};
use alloy::{primitives::U256, providers::Provider};
use eyre::Result;
use std::time::{Duration, Instant};

pub(super) async fn place(context: &Context<'_>, budget: &TokenPolicy, row: &mut Row,
    raw: U256, start: U256, floor: U256) -> Result<()> {
    row.start_out_raw = Some(start.to_string());
    row.announce_status = Some("failed".into());
    let input = PlaceInput {
        chain_id: context.input.chain_id.clone(), proxy_owner: format!("{:?}", context.owner),
        from: budget.token.clone(), to: context.receive.address.clone(), amount: raw.to_string(),
        start_out: start.to_string(), end_out: floor.to_string(), decay_secs: None,
        duration_secs: None, deadline_secs: None, relay: true, self_submit: false,
        dry_run: context.input.dry_run, max_amount: context.server_cap.map(|cap| cap.to_string()),
    };
    if !context.input.dry_run {
        row.placement_block = Some(context.provider.get_block_number().await?);
    }
    let result = match intent::place(intent::Announcer { relay: context.client, wait: context.wait },
        input, context.signer.clone(), !context.input.dry_run).await {
        Ok(result) => result,
        Err(error) => {
            let text = error.to_string();
            if text.starts_with("HTTP 422") {
                for code in ["floor_below_confirmed_discount", "unpriced_for_confirmed_discount"] {
                    if text.contains(code) { row.reason = Some(code.into()); return Ok(()); }
                }
            }
            return Err(error);
        }
    };
    row.intent_id = Some(result.id);
    row.relay = result.relay;
    row.announce_status = Some(if result.dry_run { "dry_run" } else { "accepted" }.into());
    row.outcome = if result.dry_run { "skipped" } else { "placed" }.into();
    row.reason = result.dry_run.then(|| "dry_run".into());
    Ok(())
}

pub(super) async fn wait(input: &Input, output: &mut Output) {
    let duration = Duration::from_secs(input.wait.unwrap_or(0));
    if duration.is_zero() { return; }
    let start = Instant::now();
    for row in &mut output.tokens {
        if row.announce_status.as_deref() == Some("accepted") { row.wait_timed_out = true; }
    }
    while start.elapsed() < duration {
        for row in output.tokens.iter_mut().filter(|r| r.wait_timed_out) {
            let Some(id) = row.intent_id.clone() else { continue; };
            let request = intent::StatusInput { chain_id: input.chain_id.clone(), id, lookback_blocks: None };
            match tokio::time::timeout(duration.saturating_sub(start.elapsed()), intent::status_since(request, row.placement_block)).await {
                Ok(Ok(record)) => {
                    row.wait_timed_out = !matches!(record.status.as_str(), "filled" | "expired" | "cancelled" | "dead");
                    row.intent_status = Some(record.status);
                    row.status_error = None;
                }
                Ok(Err(error)) => {
                    row.intent_status = Some("unknown".into());
                    row.status_error = Some(crate::redact::urls(&error.to_string()));
                }
                Err(_) => {
                    row.intent_status.get_or_insert_with(|| "unknown".into());
                    row.status_error = Some("status read exceeded --wait".into());
                }
            }
        }
        if !output.tokens.iter().any(|r| r.wait_timed_out) { break; }
        tokio::time::sleep(Duration::from_secs(1).min(duration.saturating_sub(start.elapsed()))).await;
    }
}
