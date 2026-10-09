// One sweep token: read spend limits and independent prices, then place an intent or trade.
// Signing and submission stay in the existing intent and trade services.
use super::{Context, Row, Via, math, now, intent_sale};
use crate::{order_types::{self, UserProxyV6}, service::{intent::TokenPolicy, portfolio::{amount, discovery}, quote, token, trade}};
use alloy::primitives::{Address, U256};
use eyre::Result;

alloy::sol! {
    #[sol(rpc)]
    contract AllowanceReader {
        function allowance(address owner, address spender) external view returns (uint256);
    }
}

pub(super) async fn sell(context: &Context<'_>, budget: &TokenPolicy, row: &mut Row) -> Result<bool> {
    let address = discovery::erc20(&budget.token)?;
    let receive = discovery::erc20(&context.receive.address)?;
    if address == receive { row.reason = Some("receive_token".into()); return Ok(false); }
    if order_types::parse_u256(&budget.cap)? == U256::ZERO {
        row.reason = Some("zero".into()); return Ok(false);
    }
    let (balance, raw) = current_amount(context, address).await?;
    row.amount_raw = raw.to_string();
    if raw == U256::ZERO { row.reason = Some("zero".into()); return Ok(false); }
    let config = discovery::config(&context.input.chain_id)?;
    let from = token::read_metadata(&context.provider, address, config.id).await?;
    let input_price = context.prices.get(&address);
    let output_price = context.prices.get(&receive);
    row.value_usd = input_price.map(|p| amount::valuation(raw, p.value, from.decimals, None).0);
    if let Some(reason) = math::price_skip(balance, from.decimals, input_price, output_price, context.max) {
        row.reason = Some(reason.into()); return Ok(false);
    }
    let floor = math::floor(raw, input_price.ok_or_else(|| eyre::eyre!("unpriced"))?, output_price.ok_or_else(|| eyre::eyre!("unpriced"))?,
        from.decimals, context.receive.decimals, context.input.max_loss_bps)?;
    row.floor_raw = Some(floor.to_string());
    if floor == U256::ZERO { row.reason = Some("below_floor".into()); return Ok(false); }
    if context.input.via == Via::Intent {
        let start = math::floor(raw, input_price.ok_or_else(|| eyre::eyre!("unpriced"))?, output_price.ok_or_else(|| eyre::eyre!("unpriced"))?,
            from.decimals, context.receive.decimals, 0)?;
        intent_sale::place(context, budget, row, raw, start, floor).await?;
        return Ok(false);
    }
    market(context, budget, row, raw, floor).await
}

async fn market(context: &Context<'_>, budget: &TokenPolicy, row: &mut Row, raw: U256, floor: U256) -> Result<bool> {
    let request = quote::QuoteInput { chain_id: context.input.chain_id.clone(), from: budget.token.clone(), to: context.receive.address.clone(), amount: raw.to_string(), slippage: Some(context.input.max_loss_bps), verify: false };
    let checked = match quote::quote(context.client, request).await {
        Ok(checked) => checked,
        Err(error) => {
            let message = error.to_string().to_ascii_lowercase();
            if message.starts_with("http 404 ") && (message.contains("no executable route") || message.contains("no route")) {
                row.reason = Some("no_route".into()); return Ok(false);
            }
            row.reason = Some("quote_failed".into()); return Err(error);
        }
    };
    let quoted = checked.response["output"].as_str().and_then(|s| order_types::parse_raw_amount("quote output", s).ok());
    row.quote_out_raw = quoted.map(|q| q.to_string());
    if let Some(reason) = math::quote_skip(quoted, floor) { row.reason = Some(reason.into()); return Ok(false); }
    let body = serde_json::json!({"chain_id":checked.request.chain_id, "token_in":checked.request.token_in,
        "token_out":checked.request.token_out, "amount_in":checked.request.amount_in,
        "slippage_bps":context.input.max_loss_bps});
    let client = context.client.clone().with_pinned_quote(body, checked.response);
    let input = trade_input(context, budget, raw, floor);
    Ok(row.record(trade::execute_trade(&client, context.signer.clone(), input, !context.input.dry_run, context.wait).await))
}

async fn current_amount(context: &Context<'_>, token: Address) -> Result<(U256, U256)> {
    let proxy = order_types::parse_address(&context.input.proxy)?;
    let contract = UserProxyV6::new(proxy, context.provider.clone());
    let policy = contract.policyOf(context.signer.address()).call().await?;
    let now = now(&context.provider).await?;
    eyre::ensure!(policy.expiry > now, "agent policy is inactive");
    context.input.via.require_action(policy.actionMask)?;
    eyre::ensure!(policy.generation.to_string() == context.policy.generation, "agent policy changed during sweep");
    let info = contract.agentTokenInfo(context.signer.address(), token).call().await?;
    if !info.allowed { return Ok((U256::ZERO, U256::ZERO)); }
    let remaining = math::remaining(info.cap, info.used, info.epochStart, u64::from(policy.epochLen), now);
    let balance = discovery::balance(&context.provider, token, context.owner).await?;
    let allowance = AllowanceReader::new(token, context.provider.clone()).allowance(context.owner, proxy).call().await?;
    Ok((balance, math::spendable(balance, remaining, allowance)))
}

pub(super) fn trade_input(context: &Context<'_>, budget: &TokenPolicy, raw: U256, floor: U256) -> trade::TradeInput {
    trade::TradeInput { chain_id: context.input.chain_id.clone(), from: budget.token.clone(), to: context.receive.address.clone(),
        amount: raw.to_string(), min_out: Some(floor.to_string()), max_amount: Some(context.server_cap.map_or(raw, |cap| cap.min(raw)).to_string()),
        slippage: Some(context.input.max_loss_bps), mode: "agent-order".into(), proxy: context.input.proxy.clone(), nonce: None, deadline_secs: None,
        dry_run: context.input.dry_run, self_submit: context.input.self_submit, verify_quote: false }
}
