// MCP stdio server exposing agent-facing AgentSwap tools.
// Exports: Config and serve_stdio.
// Deps: rmcp, crate::{client, service, signer}.

use crate::client::Client;
use crate::service::{intent, market, quote, trade};
use crate::signer::Signer;
use crate::tokens::chain_name_to_id;
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
        Self {
            tool_router: Self::tool_router(),
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
    #[tool(description = "Get a swap quote by chain ID; aliases such as base are accepted as a convenience. Amount is an unsigned decimal integer in the input token's smallest unit.")]
    async fn quote(
        &self,
        Parameters(input): Parameters<quote::QuoteInput>,
    ) -> std::result::Result<Json<quote::QuoteOutput>, String> {
        quote::quote(&self.client, input)
            .await
            .map(Json)
            .map_err(|e| format!("{e}"))
    }

    #[tool(description = "Get quotes for multiple FROM/TO pairs by chain ID; aliases such as base are accepted as a convenience. Amount is an unsigned decimal integer in the input token's smallest unit.")]
    async fn batch_quote(
        &self,
        Parameters(input): Parameters<BatchQuoteInput>,
    ) -> std::result::Result<Json<BatchQuoteOutput>, String> {
        quote::batch_quote(&self.client, &input.chain_id, &input.pairs, &input.amount)
            .await
            .map(|results| Json(BatchQuoteOutput { results }))
            .map_err(|e| format!("{e}"))
    }

    #[tool(description = "List supported tokens")]
    async fn tokens(
        &self,
        Parameters(input): Parameters<TokensInput>,
    ) -> std::result::Result<Json<ValueOutput>, String> {
        if let Some(chain_id) = input.chain_id.as_deref() {
            chain_name_to_id(chain_id).ok_or_else(|| {
                format!("unknown chain id: {chain_id}. Pass a chain ID such as 8453 (aliases like base are accepted)")
            })?;
        }
        match market::tokens(&self.client).await {
            Ok(tokens) => filter_tokens(tokens, input.chain_id.as_deref())
                .map(|value| Json(ValueOutput { value })),
            Err(e) => Err(format!("{e}")),
        }
    }

    #[tool(description = "Inspect a pool by chain ID and address; aliases such as base are accepted as a convenience.")]
    async fn pools(
        &self,
        Parameters(input): Parameters<PoolsInput>,
    ) -> std::result::Result<Json<ValueOutput>, String> {
        market::pool(&self.client, &input.chain_id, &input.address)
            .await
            .map(|value| Json(ValueOutput { value }))
            .map_err(|e| format!("{e}"))
    }

    #[tool(description = "Quote and sign an AgentOrder; amount, min_out, and max_amount are unsigned decimal integers in raw token units. Dry-run defaults to true and requires a reachable RPC and deployed V6 proxy to verify policy and order hashes before signing")]
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
        trade::execute_trade(&self.client, signer, input, self.allow_trade)
            .await
            .map(Json)
            .map_err(|e| format!("{e}"))
    }

    #[tool(description = "Sign and announce a V6 open intent; amount, start_out, end_out, and max_amount are unsigned decimal integers in raw token units. Dry-run is forced without allow_trade")]
    async fn intent_place(
        &self,
        Parameters(mut input): Parameters<intent::PlaceInput>,
    ) -> std::result::Result<Json<intent::PlaceOutcome>, String> {
        if !self.allow_trade { input.dry_run = true; }
        bound_intent_cap(&mut input, self.trade_max_amount.as_deref())?;
        let Some(signer) = self.signer.clone() else { return Err("intent_place requires --key-file".to_string()); };
        intent::place(&self.intent_client, input, signer, self.allow_trade)
            .await.map(Json).map_err(|e| format!("{e}"))
    }

    #[tool(description = "List announced V6 intents by owner or agent")]
    async fn intent_list(
        &self,
        Parameters(input): Parameters<intent::ListInput>,
    ) -> std::result::Result<Json<IntentListOutput>, String> {
        if input.owner.is_none() && input.agent.is_none() {
            return Err("intent_list requires owner or agent".to_string());
        }
        intent::list(input).await.map(|intents| Json(IntentListOutput { intents })).map_err(|e| format!("{e}"))
    }

    #[tool(description = "Inspect one V6 intent by bytes32 id")]
    async fn intent_status(
        &self,
        Parameters(input): Parameters<intent::StatusInput>,
    ) -> std::result::Result<Json<intent::IntentRecord>, String> {
        intent::status(input).await.map(Json).map_err(|e| format!("{e}"))
    }

    #[tool(description = "Read a V6 agent policy and per-token cap/usage")]
    async fn policy(
        &self,
        Parameters(input): Parameters<intent::PolicyInput>,
    ) -> std::result::Result<Json<intent::PolicyOutput>, String> {
        intent::policy(input).await.map(Json).map_err(|e| format!("{e}"))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AgentSwapMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("AgentSwap tools: quote, batch_quote, tokens, pools, trade, intent_place, intent_list, intent_status, policy.")
    }
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
    /// Chain ID such as 8453; known aliases such as base are also accepted.
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
    /// Chain ID such as 8453; known aliases such as base are also accepted.
    chain_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PoolsInput {
    /// Chain ID such as 8453; known aliases such as base are also accepted.
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
    let chain_id = chain_name_to_id(chain_id).ok_or_else(|| format!("unknown chain id: {chain_id}. Pass a chain ID such as 8453 (aliases like base are accepted)"))?;
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
