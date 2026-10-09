// Sequential grant-bounded sales with independent app-price floors.
// Uses the existing policy and trade services; stops after any unconfirmed broadcast.
mod math;
mod models;
mod sale;
#[cfg(test)]
mod tests;
pub use models::{Input, Output, Row};
use crate::{client::Client, evm, order_types::{self, UserProxyV6}, service::{intent, portfolio::{amount, discovery, prices}, token, submit::Wait}, signer::Signer};
use alloy::{primitives::Address, providers::{DynProvider, Provider}};
use eyre::{Result, eyre};
use std::{collections::BTreeMap, sync::Arc};

struct Context<'a> {
    client: &'a Client,
    signer: Arc<dyn Signer>,
    input: Input,
    provider: DynProvider,
    owner: Address,
    receive: token::Token,
    prices: BTreeMap<Address, prices::Price>,
    policy: intent::PolicyOutput,
    max: alloy::primitives::U256,
    server_cap: Option<alloy::primitives::U256>,
    wait: Wait,
}

pub async fn sweep(client: &Client, signer: Arc<dyn Signer>, mut input: Input, allow: bool, cap: Option<&str>, wait: Wait) -> Result<Output> {
    input.dry_run |= !allow;
    let max = amount::fixed(&input.max_usd, false)?;
    eyre::ensure!(input.max_loss_bps <= 10_000, "max-loss-bps must be at most 10000");
    let server_cap = cap.map(|c| order_types::parse_raw_amount("sweep max-amount", c)).transpose()?;
    let config = discovery::config(&input.chain_id)?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    let proxy = discovery::erc20(&input.proxy)?;
    let owner = UserProxyV6::new(proxy, provider.clone()).owner().call().await?;
    let policy = intent::policy(intent::PolicyInput { chain_id: input.chain_id.clone(), owner: format!("{owner:?}"),
        agent: format!("{:?}", signer.address()), lookback_blocks: input.lookback_blocks, tokens: input.tokens.clone() }).await?;
    eyre::ensure!(order_types::parse_address(&policy.proxy)? == proxy, "proxy differs from owner's factory proxy");
    let receive = token::resolve(&input.receive, config.id).await?;
    validate_policy(&policy, discovery::erc20(&receive.address)?, now(&provider).await?)?;
    let addresses = policy.tokens.iter().filter(|t| t.allowed).map(|t| discovery::erc20(&t.token)).collect::<Result<Vec<_>>>()?;
    let prices = prices::prices(config.id, &addresses, crate::routes::APP_ORIGIN).await;
    // Preview and route discovery must not sign x402 payments, including forced dry runs.
    let client = client.clone().with_x402(crate::x402::Config::disabled(), None);
    run(Context { client: &client, signer, input, provider, owner, receive, prices, policy, max, server_cap, wait }).await
}

fn validate_policy(policy: &intent::PolicyOutput, receive: Address, now: u64) -> Result<()> {
    eyre::ensure!(policy.expiry.parse::<u64>()? > now, "agent policy is expired");
    eyre::ensure!(policy.action_mask.parse::<u8>()? & 1 != 0, "agent policy lacks market action");
    eyre::ensure!(policy.tokens.iter().any(|t| t.allowed && t.token.parse::<Address>().ok() == Some(receive)), "receive token is not in the basket");
    Ok(())
}

async fn now(provider: &DynProvider) -> Result<u64> {
    Ok(provider.get_block_by_number(alloy::eips::BlockNumberOrTag::Latest).await?
        .ok_or_else(|| eyre!("latest block unavailable"))?.header.timestamp)
}

async fn run(context: Context<'_>) -> Result<Output> {
    let mut output = Output { dry_run: context.input.dry_run, owner: format!("{:?}", context.owner), note: context.policy.note.clone(), tokens: Vec::new() };
    let mut stopped = false;
    for budget in context.policy.tokens.iter().filter(|t| t.allowed) {
        let mut row = Row::new(budget.token.clone());
        if stopped { row.reason = Some("sweep_stopped".into()); }
        else {
            match sale::sell(&context, budget, &mut row).await {
                Ok(stop) => stopped = stop,
                Err(error) => { row.record(Err(error)); }
            }
        }
        output.tokens.push(row);
    }
    Ok(output)
}

fn default_true() -> bool { true }
