// Sweep relayed placements reuse the existing intent service and keep each signed order for
// status reads. Price math and spend limits are checked by sale before this signing boundary.
use super::{Context, Row};
use crate::{order_types::{Order, OrderDto, parse_address, parse_u256}, service::intent::{self, PlaceInput, TokenPolicy}};
use alloy::{primitives::U256, providers::Provider};
use eyre::Result;

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
    let result = match intent::place(intent::Announcer { relay: context.client, wait: context.wait, pacer: Some(&context.pacer) },
        input, context.signer.clone(), !context.input.dry_run).await {
        Ok(result) => result,
        Err(error) => {
            if error.downcast_ref::<crate::client::RateLimited>().is_some() { row.reason = Some("relay_rate_limited".into()); }
            let text = error.to_string();
            if text.starts_with("HTTP 422") {
                for code in ["floor_below_confirmed_discount", "unpriced_for_confirmed_discount"] {
                    if text.contains(code) { row.reason = Some(code.into()); return Ok(()); }
                }
            }
            return Err(error);
        }
    };
    row.order = Some(order(&result.order)?);
    row.intent_id = Some(result.id);
    row.relay = result.relay;
    row.announce_status = Some(if result.dry_run { "dry_run" } else { "accepted" }.into());
    row.outcome = if result.dry_run { "skipped" } else { "placed" }.into();
    row.reason = result.dry_run.then(|| "dry_run".into());
    Ok(())
}

/// The signed order back from its DTO, for lens status reads.
pub(super) fn order(dto: &OrderDto) -> Result<Order> {
    Ok(Order { owner: parse_address(&dto.owner)?, recipient: parse_address(&dto.recipient)?,
        tokenIn: parse_address(&dto.token_in)?, amountIn: parse_u256(&dto.amount_in)?,
        tokenOut: parse_address(&dto.token_out)?, startAmountOut: parse_u256(&dto.start_amount_out)?,
        endAmountOut: parse_u256(&dto.end_amount_out)?, startTime: parse_u256(&dto.start_time)?,
        decayEndTime: parse_u256(&dto.decay_end_time)?, endTime: parse_u256(&dto.end_time)?,
        appData: dto.app_data.parse()?, nonce: parse_u256(&dto.nonce)? })
}
