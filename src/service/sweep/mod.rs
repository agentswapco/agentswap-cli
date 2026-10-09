// Sequential grant-bounded sales with independent app-price floors.
// Uses the existing policy and trade services; stops after any unconfirmed broadcast.
mod math;
mod models;
mod sale;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod policy_tests;
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
    amount::fixed(&input.max_usd, false)?;
    eyre::ensure!(input.max_loss_bps < 10_000, "max-loss-bps must be less than 10000");
    eyre::ensure!(!input.tokens.is_empty(), "sweep requires at least one --token (MCP tokens)");
    let server_cap = cap.map(|c| order_types::parse_raw_amount("sweep max-amount", c)).transpose()?;
    let config = discovery::config(&input.chain_id)?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    read(client, signer, input, provider, crate::routes::APP_ORIGIN, server_cap, wait).await
}

async fn read(client: &Client, signer: Arc<dyn Signer>, input: Input, provider: DynProvider,
    app_origin: &str, server_cap: Option<alloy::primitives::U256>, wait: Wait) -> Result<Output> {
    let config = discovery::config(&input.chain_id)?;
    let max = amount::fixed(&input.max_usd, false)?;
    let proxy = discovery::erc20(&input.proxy)?;
    let owner = UserProxyV6::new(proxy, provider.clone()).owner().call().await?;
    let receive = token::resolve(&input.receive, config.id).await?;
    let policy = read_policy(&provider, &input, owner, signer.address(), discovery::erc20(&receive.address)?).await?;
    validate_policy(&policy, discovery::erc20(&receive.address)?, now(&provider).await?)?;
    let addresses = policy.tokens.iter().filter(|t| t.allowed).map(|t| discovery::erc20(&t.token)).collect::<Result<Vec<_>>>()?;
    let prices = prices::prices(config.id, &addresses, app_origin).await;
    // A forced or explicit preview must not sign x402 payments.
    let client = if input.dry_run { client.clone().with_x402(crate::x402::Config::disabled(), None) } else { client.clone() };
    run(Context { client: &client, signer, input, provider, owner, receive, prices, policy, max, server_cap, wait }).await
}

async fn read_policy(provider: &DynProvider, input: &Input, owner: Address, agent: Address, receive: Address) -> Result<intent::PolicyOutput> {
    let config = discovery::config(&input.chain_id)?;
    let address = discovery::erc20(&input.proxy)?;
    let factory_proxy = order_types::UserProxyFactoryV6::new(config.factory, provider.clone()).proxyOf(owner).call().await?;
    eyre::ensure!(factory_proxy == address, "proxy differs from owner's factory proxy");
    let proxy = UserProxyV6::new(address, provider.clone());
    let policy = proxy.policyOf(agent).call().await?;
    let mut addresses = input.tokens.iter().map(|t| discovery::erc20(t)).collect::<Result<std::collections::BTreeSet<_>>>()?;
    addresses.insert(receive);
    let mut tokens = Vec::new();
    for token in addresses {
        let info = proxy.agentTokenInfo(agent, token).call().await?;
        tokens.push(intent::TokenPolicy { token: format!("{token:?}"), allowed: info.allowed,
            cap: info.cap.to_string(), used: info.used.to_string(), epoch_start: info.epochStart.to_string() });
    }
    Ok(intent::PolicyOutput { owner: format!("{owner:?}"), agent: format!("{agent:?}"), proxy: format!("{address:?}"),
        expiry: policy.expiry.to_string(), epoch_len: policy.epochLen.to_string(), action_mask: policy.actionMask.to_string(),
        generation: policy.generation.to_string(), tokens, note: None })
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
