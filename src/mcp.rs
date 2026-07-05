// MCP stdio server exposing agent-facing AgentSwap tools.
// Exports: Config and serve_stdio.
// Deps: rmcp, crate::{client, service, signer}.

use crate::client::Client;
use crate::service::{market, quote, trade};
use crate::signer::Signer;
use crate::tokens::chain_name_to_id;
use eyre::Result;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, Json, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Clone)]
pub struct Config {
    pub client: Client,
    pub signer: Option<Arc<dyn Signer>>,
    pub allow_trade: bool,
    /// Operator-set per-trade notional cap; bounds any client-supplied cap.
    pub trade_max_amount: Option<String>,
}

#[derive(Clone)]
struct AgentSwapMcp {
    tool_router: ToolRouter<Self>,
    client: Client,
    signer: Option<Arc<dyn Signer>>,
    allow_trade: bool,
    trade_max_amount: Option<String>,
}

impl AgentSwapMcp {
    fn new(config: Config) -> Self {
        Self {
            tool_router: Self::tool_router(),
            client: config.client,
            signer: config.signer,
            allow_trade: config.allow_trade,
            trade_max_amount: config.trade_max_amount,
        }
    }
}

#[tool_router(router = tool_router)]
impl AgentSwapMcp {
    #[tool(description = "Get a swap quote")]
    async fn quote(
        &self,
        Parameters(input): Parameters<quote::QuoteInput>,
    ) -> std::result::Result<Json<quote::QuoteOutput>, String> {
        quote::quote(&self.client, input)
            .await
            .map(Json)
            .map_err(|e| format!("{e}"))
    }

    #[tool(description = "Get quotes for multiple FROM/TO pairs")]
    async fn batch_quote(
        &self,
        Parameters(input): Parameters<BatchQuoteInput>,
    ) -> std::result::Result<Json<Vec<quote::BatchQuoteResult>>, String> {
        quote::batch_quote(&self.client, &input.chain, &input.pairs, &input.amount)
            .await
            .map(Json)
            .map_err(|e| format!("{e}"))
    }

    #[tool(description = "List supported tokens")]
    async fn tokens(
        &self,
        Parameters(input): Parameters<TokensInput>,
    ) -> std::result::Result<Json<serde_json::Value>, String> {
        match market::tokens(&self.client).await {
            Ok(tokens) => filter_tokens(tokens, input.chain.as_deref()).map(Json),
            Err(e) => Err(format!("{e}")),
        }
    }

    #[tool(description = "Inspect a pool by chain and address")]
    async fn pools(
        &self,
        Parameters(input): Parameters<PoolsInput>,
    ) -> std::result::Result<Json<serde_json::Value>, String> {
        market::pool(&self.client, &input.chain, &input.address)
            .await
            .map(Json)
            .map_err(|e| format!("{e}"))
    }

    #[tool(description = "Quote and sign an AgentOrder; dry-run defaults to true")]
    async fn trade(
        &self,
        Parameters(mut input): Parameters<trade::TradeInput>,
    ) -> std::result::Result<Json<trade::TradeOutcome>, String> {
        if !self.allow_trade {
            input.dry_run = true;
        }
        // Bound the client-supplied per-trade cap by the operator's server cap, if set.
        if let Some(server_cap) = &self.trade_max_amount {
            let tighter = match &input.max_amount {
                Some(client_cap) => {
                    let c = client_cap.parse::<u128>().unwrap_or(u128::MAX);
                    let s = server_cap.parse::<u128>().unwrap_or(u128::MAX);
                    if s < c { server_cap.clone() } else { client_cap.clone() }
                }
                None => server_cap.clone(),
            };
            input.max_amount = Some(tighter);
        }
        let Some(signer) = self.signer.clone() else {
            return Err("trade requires --key-file".to_string());
        };
        trade::execute_trade(&self.client, &self.client, signer, input, self.allow_trade)
            .await
            .map(Json)
            .map_err(|e| format!("{e}"))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AgentSwapMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("AgentSwap tools: quote, batch_quote, tokens, pools, trade.")
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct BatchQuoteInput {
    chain: String,
    pairs: Vec<String>,
    amount: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TokensInput {
    chain: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PoolsInput {
    chain: String,
    address: String,
}

pub async fn serve_stdio(config: Config) -> Result<()> {
    let service = AgentSwapMcp::new(config)
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

fn filter_tokens(tokens: serde_json::Value, chain: Option<&str>) -> std::result::Result<serde_json::Value, String> {
    let Some(chain) = chain else {
        return Ok(tokens);
    };
    let chain_id = chain_name_to_id(chain).ok_or_else(|| format!("unknown chain: {chain}"))?;
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
