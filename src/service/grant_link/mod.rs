// Advisory grant URL construction and live-policy replacement checks, shared by CLI and MCP.
// Reads only RPC metadata, balances and V6 policy; never signs or submits transactions.
mod url;
#[cfg(test)]
mod tests;
use crate::{evm, order_types::{self, UserProxyFactoryV6, UserProxyV6}, service::{portfolio::{amount, discovery}, token}};
use alloy::{primitives::Address, providers::{DynProvider, Provider}};
use eyre::{Result, eyre};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Debug, Clone, Deserialize, JsonSchema, clap::Args)]
pub struct Input {
    #[arg(long = "chainid", help = crate::tokens::CHAIN_ID_HELP)]
    #[schemars(description = crate::tokens::CHAIN_ID_HELP)]
    pub chain_id: String,
    /// Owner wallet that reviews the grant in the app.
    #[arg(long)]
    pub owner: String,
    /// Agent wallet to authorize.
    #[arg(long)]
    pub agent: String,
    /// Spend address optionally followed by :raw-cap; repeatable. Recurring grants require caps.
    #[arg(long = "token", required = true)]
    pub tokens: Vec<String>,
    /// Receive-only token symbol or address, appended with a zero cap.
    #[arg(long)]
    pub receive: String,
    /// Use omitted caps from current balances, a weekly epoch and expiry in 24 hours.
    #[arg(long, conflicts_with_all = ["epoch", "expiry"])]
    #[serde(default)]
    pub one_shot: bool,
    /// Recurring cap period: 1h, 1d or 1w.
    #[arg(long, required_unless_present = "one_shot", requires = "expiry")]
    pub epoch: Option<String>,
    /// Recurring expiry: 7d, 30d, 90d or a future ISO timestamp in UTC ending Z.
    #[arg(long, required_unless_present = "one_shot", requires = "epoch")]
    pub expiry: Option<String>,
    /// Label included in the advisory link.
    #[arg(long)]
    pub label: Option<String>,
    /// Note included in the advisory link.
    #[arg(long)]
    pub note: Option<String>,
    /// Permit replacement of a live policy; output describes the replaced policy.
    #[arg(long)]
    #[serde(default)]
    pub replace: bool,
    #[arg(long, value_parser = ["batch-sell"], requires = "max_loss_bps")]
    pub purpose: Option<String>,
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=5000), requires = "purpose")]
    #[schemars(range(min = 1, max = 5000))]
    pub max_loss_bps: Option<u16>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TokenSummary {
    pub address: String,
    pub symbol: String,
    pub decimals: u8,
    pub raw: String,
    pub human: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct Policy {
    pub proxy: String,
    pub expiry: String,
    pub epoch_len: String,
    pub action_mask: String,
    pub generation: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct Output {
    pub url: String,
    pub tokens: Vec<TokenSummary>,
    pub warning: String,
    pub replaced_policy: Option<Policy>,
}

pub async fn grant_link(input: Input) -> Result<Output> {
    let config = discovery::config(&input.chain_id)?;
    let provider = evm::read_provider(&evm::rpc_url(config))?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    read(input, &provider, now).await
}

pub(crate) async fn read(input: Input, provider: &DynProvider, now: u64) -> Result<Output> {
    url::purpose(&input)?;
    let config = discovery::config(&input.chain_id)?;
    let owner = order_types::parse_address(&input.owner)?;
    let agent = order_types::parse_address(&input.agent)?;
    let (epoch, expiry) = url::schedule(&input, now)?;
    let receive = token::from_registry(&input.receive, config.id).map(|t| t.address).unwrap_or_else(|| input.receive.clone());
    let requests = url::requests(&input, &receive)?;
    let replaced_policy = policy(provider, config, owner, agent, now, input.replace).await?;
    let mut tokens = Vec::new();
    for (address, cap) in requests {
        let metadata = token::read_metadata(provider, address, config.id).await?;
        let raw = match cap { Some(cap) => cap, None => discovery::balance(provider, address, owner).await?.to_string() };
        let human = amount::render(&raw, metadata.decimals as usize);
        if human.len() > 32 { return Err(eyre!("rendered cap exceeds 32 characters for {address:?}")); }
        tokens.push(TokenSummary { address: format!("{address:?}"), symbol: metadata.symbol, decimals: metadata.decimals, raw, human });
    }
    if !tokens.iter().any(|t| t.raw != "0") { return Err(eyre!("at least one positive spend cap is required")); }
    let mut link = url::build(config.id, &format!("{agent:?}"), Some(&format!("{owner:?}")), input.label.as_deref(), input.note.as_deref(), &tokens, &epoch, &expiry)?;
    if let Some(loss) = input.max_loss_bps {
        let mut url = reqwest::Url::parse(&link)?;
        url.query_pairs_mut().append_pair("purpose", "batch-sell").append_pair("maxloss", &loss.to_string());
        link = url.into();
    }
    let warning = if replaced_policy.is_some() { "This link replaces the live policy's entire token basket. The owner must review and approve in the app." }
        else { "Advisory link: the app re-reads token metadata and the owner reviews and approves the grant." }.into();
    Ok(Output { url: link, tokens, warning, replaced_policy })
}

async fn policy(provider: &DynProvider, config: evm::ChainConfig, owner: Address, agent: Address, now: u64, replace: bool) -> Result<Option<Policy>> {
    let proxy = UserProxyFactoryV6::new(config.factory, provider.clone()).proxyOf(owner).call().await?;
    if proxy == Address::ZERO || provider.get_code_at(proxy).await?.is_empty() { return Ok(None); }
    let policy = UserProxyV6::new(proxy, provider.clone()).policyOf(agent).call().await?;
    if policy.expiry <= now || policy.actionMask == 0 { return Ok(None); }
    if !replace { return Err(eyre!("agent has a live policy (expiry {}, action mask {}, generation {}); use --replace to replace its entire basket", policy.expiry, policy.actionMask, policy.generation)); }
    Ok(Some(Policy { proxy: format!("{proxy:?}"), expiry: policy.expiry.to_string(), epoch_len: policy.epochLen.to_string(), action_mask: policy.actionMask.to_string(), generation: policy.generation.to_string() }))
}
