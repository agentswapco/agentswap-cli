// MCP stdio server exposing agent-facing AgentSwap tools.
// Exports: Config and serve_stdio.
// Deps: rmcp, crate::{client, redact, service, signer}.

use crate::client::Client;
use crate::service::submit::{NotConfirmed, Wait};
use crate::service::{intent, market, quote, trade, portfolio, grant_link};
use crate::signer::Signer;
use crate::tokens::{chain_name_to_id, unknown_chain_id, CHAIN_ID_HELP};
use eyre::Result;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, Json, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod portfolio_tests;
#[cfg(test)]
mod cli_dry_run_tests;
#[cfg(test)]
mod dry_run_fixture;
#[cfg(test)]
mod dry_run_tests;

#[derive(Clone)]
pub struct Config {
    pub client: Client,
    pub intent_client: Client,
    pub signer: Option<Arc<dyn Signer>>,
    pub allow_trade: bool,
    /// Operator-set per-trade notional cap; bounds any client-supplied cap.
    pub trade_max_amount: Option<String>,
}

#[derive(Clone)]
struct AgentSwapMcp {
    tool_router: ToolRouter<Self>,
    client: Client,
    intent_client: Client,
    signer: Option<Arc<dyn Signer>>,
    allow_trade: bool,
    trade_max_amount: Option<String>,
}

impl AgentSwapMcp {
    fn new(config: Config) -> Self {
        let mut tool_router = Self::tool_router();
        for route in tool_router.map.values_mut().filter(|route| matches!(
            route.attr.name.as_ref(), "trade" | "intent_place" | "intent_list" | "intent_status" | "policy"
        )) {
            route.attr.description = Some(format!("{} {}",
                route.attr.description.as_deref().unwrap_or_default(), crate::tokens::V6_CHAINS_NOTE).into());
        }
        for route in tool_router.map.values_mut().filter(|route| matches!(route.attr.name.as_ref(), "portfolio" | "grant_link")) {
            route.attr.description = Some(format!("{} {}", route.attr.description.as_deref().unwrap_or_default(), crate::tokens::HOLDINGS_CHAINS_NOTE).into());
        }
        Self {
            tool_router,
            client: config.client,
            intent_client: config.intent_client,
            signer: config.signer,
            allow_trade: config.allow_trade,
            trade_max_amount: config.trade_max_amount,
        }
    }
}

#[tool_router(router = tool_router)]
impl AgentSwapMcp {
    #[tool(description = "Discover ERC-20 holdings from wallet-tokens, service catalog, registry, explicit addresses and best-effort Transfer logs. App prices include provenance and floor_eligible; source failures and scanned range are reported; max_usd is an unsigned USD decimal, quotes full eligible balances and unpriced tokens. Quotes never sign payments. Older tokens may be missing.")]
    async fn portfolio(&self, Parameters(input): Parameters<portfolio::Input>) -> std::result::Result<Json<portfolio::Output>, String> {
        portfolio::portfolio(&self.client, input).await.map(Json).map_err(tool_error)
    }

    #[tool(description = "Create an advisory grant URL without signing. Requires ERC-20 spend addresses, receive token, and either one_shot or epoch plus expiry. Recurring caps are explicit raw integers; one_shot omitted caps use current balances. Rejects duplicates, native tokens, more than 20 tokens, no positive cap and human caps longer than 32 characters. Live policies require replace.")]
    async fn grant_link(&self, Parameters(input): Parameters<grant_link::Input>) -> std::result::Result<Json<grant_link::Output>, String> {
        grant_link::grant_link(input).await.map(Json).map_err(tool_error)
    }

    #[tool(description = "Get a swap quote. Amount is an unsigned decimal integer in the input token's smallest unit.")]
    async fn quote(
        &self,
        Parameters(input): Parameters<quote::QuoteInput>,
    ) -> std::result::Result<Json<quote::QuoteOutput>, String> {
        quote::quote(&self.client, input)
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(description = "Get quotes for multiple FROM/TO pairs. Amount is an unsigned decimal integer in the input token's smallest unit.")]
    async fn batch_quote(
        &self,
        Parameters(input): Parameters<BatchQuoteInput>,
    ) -> std::result::Result<Json<BatchQuoteOutput>, String> {
        quote::batch_quote(&self.client, &input.chain_id, &input.pairs, &input.amount)
            .await
            .map(|results| Json(BatchQuoteOutput { results }))
            .map_err(tool_error)
    }

    #[tool(description = "List supported tokens, optionally filtered by chain_id.")]
    async fn tokens(
        &self,
        Parameters(input): Parameters<TokensInput>,
    ) -> std::result::Result<Json<ValueOutput>, String> {
        if let Some(chain_id) = input.chain_id.as_deref() {
            chain_name_to_id(chain_id).ok_or_else(|| unknown_chain_id(chain_id))?;
        }
        match market::tokens(&self.client).await {
            Ok(tokens) => filter_tokens(tokens, input.chain_id.as_deref())
                .map(|value| Json(ValueOutput { value })),
            Err(e) => Err(tool_error(e)),
        }
    }

    #[tool(description = "Inspect a pool by chain ID and address.")]
    async fn pools(
        &self,
        Parameters(input): Parameters<PoolsInput>,
    ) -> std::result::Result<Json<ValueOutput>, String> {
        market::pool(&self.client, &input.chain_id, &input.address)
            .await
            .map(|value| Json(ValueOutput { value }))
            .map_err(tool_error)
    }

    #[tool(description = "Preview an unsigned AgentOrder or sign a live trade; amount, min_out and max_amount are unsigned decimal integers in raw token units. A live trade (dry_run=false) requires --allow-trade and min_out, because the quote server's output is not trusted as the protection floor. dry_run defaults to true and is forced true without --allow-trade. A dry-run returns the quote, unsigned AgentOrder and digest after reading policy generation and checking hash parity against a reachable RPC and deployed V6 proxy. It never signs or returns a signature or signed calldata, and never broadcasts. With self_submit, the result carries self_submit.txHash and txStatus; the receipt wait ends before common MCP request timeouts, and a reverted transaction, or one sent whose receipt was not read, is a tool error that carries the full outcome.")]
    async fn trade(
        &self,
        Parameters(mut input): Parameters<trade::TradeInput>,
    ) -> std::result::Result<Json<trade::TradeOutcome>, String> {
        if !self.allow_trade {
            input.dry_run = true;
        }
        // Bound the client-supplied per-trade cap by the operator's server cap, if set.
        if let Some(server_cap) = &self.trade_max_amount {
            bound_trade_cap(&mut input, server_cap)?;
        }
        let Some(signer) = self.signer.clone() else {
            return Err("trade requires --key-file".to_string());
        };
        let outcome = trade::execute_trade(&self.client, signer, input, self.allow_trade, Wait::MCP)
            .await
            .map_err(tool_error)?;
        let failure = outcome.not_confirmed();
        confirmed(outcome, failure)
    }

    #[tool(description = "Preview an unsigned V6 intent and authorization or sign and announce live; amount, start_out, end_out and max_amount are unsigned decimal integers in raw token units. dry_run is forced true without --allow-trade. A dry-run returns the unsigned intent, authorization and digest after on-chain intent-id and authorization-digest parity checks; it never signs, creates or returns a signature or envelope, relays or broadcasts. Signature-based authorization validation runs only live. A live announcement requires --allow-trade and exactly one of relay or self_submit. With self_submit, the result carries tx_hash and tx_status; the receipt wait ends before common MCP request timeouts, and a reverted announce, or one sent whose receipt was not read, is a tool error that carries the full outcome. Requires a reachable RPC and V6 deployment.")]
    async fn intent_place(
        &self,
        Parameters(mut input): Parameters<intent::PlaceInput>,
    ) -> std::result::Result<Json<intent::PlaceOutcome>, String> {
        if !self.allow_trade { input.dry_run = true; }
        bound_intent_cap(&mut input, self.trade_max_amount.as_deref())?;
        let Some(signer) = self.signer.clone() else { return Err("intent_place requires --key-file".to_string()); };
        let announcer = intent::Announcer { relay: &self.intent_client, wait: Wait::MCP };
        let outcome = intent::place(announcer, input, signer, self.allow_trade)
            .await.map_err(tool_error)?;
        let failure = outcome.not_confirmed();
        confirmed(outcome, failure)
    }

    #[tool(description = "List announced V6 intents on chain_id. Either owner or agent is required; a call with neither is refused.")]
    async fn intent_list(
        &self,
        Parameters(input): Parameters<intent::ListInput>,
    ) -> std::result::Result<Json<IntentListOutput>, String> {
        if input.owner.is_none() && input.agent.is_none() {
            return Err("intent_list requires owner or agent".to_string());
        }
        intent::list(input).await.map(|intents| Json(IntentListOutput { intents })).map_err(tool_error)
    }

    #[tool(description = "Inspect one V6 intent by bytes32 id on chain_id.")]
    async fn intent_status(
        &self,
        Parameters(input): Parameters<intent::StatusInput>,
    ) -> std::result::Result<Json<intent::IntentRecord>, String> {
        intent::status(input).await.map(Json).map_err(tool_error)
    }

    #[tool(description = "Read a V6 agent policy and per-token cap/usage on chain_id. Token addresses are discovered from cap events inside the log lookback and current budgets are read from the proxy; pass tokens to include addresses older than the lookback.")]
    async fn policy(
        &self,
        Parameters(input): Parameters<intent::PolicyInput>,
    ) -> std::result::Result<Json<intent::PolicyOutput>, String> {
        intent::policy(input).await.map(Json).map_err(tool_error)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AgentSwapMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("AgentSwap tools: quote, batch_quote, tokens, pools, trade, intent_place, intent_list, intent_status, policy, portfolio, grant_link. Portfolio and grant_link are read-only; grant links require owner review in the app.")
    }
}

/// Tool error text, with RPC and service URLs reduced to their host.
fn tool_error(error: eyre::Report) -> String {
    crate::redact::urls(&format!("{error}"))
}

/// A sent transaction that did not confirm is a tool error; its text carries the full outcome,
/// hash included.
fn confirmed<T: Serialize>(outcome: T, failure: Option<NotConfirmed>) -> std::result::Result<Json<T>, String> {
    let Some(failure) = failure else { return Ok(Json(outcome)); };
    let detail = serde_json::to_string(&outcome).unwrap_or_default();
    Err(format!("{failure}\n{detail}"))
}

fn bound_intent_cap(input: &mut intent::PlaceInput, server_cap: Option<&str>) -> std::result::Result<(), String> {
    let Some(server_cap) = server_cap else { return Ok(()); };
    let server = crate::order_types::parse_raw_amount("MCP intent max-amount", server_cap).map_err(|e| format!("{e}"))?;
    let client = input.max_amount.as_deref().map(|value| crate::order_types::parse_raw_amount("MCP intent max-amount", value)).transpose().map_err(|e| format!("{e}"))?;
    input.max_amount = Some(client.map_or_else(|| server_cap.to_string(), |value| value.min(server).to_string()));
    Ok(())
}

fn bound_trade_cap(input: &mut trade::TradeInput, server_cap: &str) -> std::result::Result<(), String> {
    let server = crate::order_types::parse_raw_amount("MCP trade max-amount", server_cap).map_err(|e| format!("{e}"))?;
    let client = input.max_amount.as_deref().map(|value| crate::order_types::parse_raw_amount("MCP trade max-amount", value)).transpose().map_err(|e| format!("{e}"))?;
    input.max_amount = Some(client.map_or_else(|| server_cap.to_string(), |value| value.min(server).to_string()));
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
struct BatchQuoteInput {
    #[schemars(description = CHAIN_ID_HELP)]
    chain_id: String,
    pairs: Vec<String>,
    /// Unsigned decimal amount in the input token's smallest unit.
    amount: String,
}

#[derive(Debug, Serialize, JsonSchema)]
struct BatchQuoteOutput {
    results: Vec<quote::BatchQuoteResult>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct IntentListOutput {
    intents: Vec<intent::IntentRecord>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct ValueOutput {
    value: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TokensInput {
    #[schemars(description = CHAIN_ID_HELP)]
    chain_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PoolsInput {
    #[schemars(description = CHAIN_ID_HELP)]
    chain_id: String,
    address: String,
}

pub async fn serve_stdio(config: Config) -> Result<()> {
    let service = AgentSwapMcp::new(config)
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

fn filter_tokens(tokens: serde_json::Value, chain_id: Option<&str>) -> std::result::Result<serde_json::Value, String> {
    let Some(chain_id) = chain_id else {
        return Ok(tokens);
    };
    let chain_id = chain_name_to_id(chain_id).ok_or_else(|| unknown_chain_id(chain_id))?;
    let Some(object) = tokens.as_object() else {
        return Ok(tokens);
    };
    let filtered = object
        .iter()
        .filter(|(_, token)| token["chain_id"].as_u64() == Some(chain_id))
        .map(|(addr, token)| (addr.clone(), token.clone()))
        .collect();
    Ok(serde_json::Value::Object(filtered))
}
