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
    ) -> std::result::Result<Json<BatchQuoteOutput>, String> {
        quote::batch_quote(&self.client, &input.chain, &input.pairs, &input.amount)
            .await
            .map(|results| Json(BatchQuoteOutput { results }))
            .map_err(|e| format!("{e}"))
    }

    #[tool(description = "List supported tokens")]
    async fn tokens(
        &self,
        Parameters(input): Parameters<TokensInput>,
    ) -> std::result::Result<Json<ValueOutput>, String> {
        match market::tokens(&self.client).await {
            Ok(tokens) => filter_tokens(tokens, input.chain.as_deref())
                .map(|value| Json(ValueOutput { value })),
            Err(e) => Err(format!("{e}")),
        }
    }

    #[tool(description = "Inspect a pool by chain and address")]
    async fn pools(
        &self,
        Parameters(input): Parameters<PoolsInput>,
    ) -> std::result::Result<Json<ValueOutput>, String> {
        market::pool(&self.client, &input.chain, &input.address)
            .await
            .map(|value| Json(ValueOutput { value }))
            .map_err(|e| format!("{e}"))
    }

    #[tool(description = "Quote and sign an AgentOrder; dry-run defaults to true and requires a reachable RPC and deployed V6 proxy to verify policy and order hashes before signing")]
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

    #[tool(description = "Sign and announce a V6 open intent; dry-run is forced without allow_trade")]
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
    let server = crate::order_types::parse_u256(server_cap).map_err(|e| format!("{e}"))?;
    let client = input.max_amount.as_deref().map(crate::order_types::parse_u256).transpose().map_err(|e| format!("{e}"))?;
    input.max_amount = Some(client.map_or_else(|| server_cap.to_string(), |value| value.min(server).to_string()));
    Ok(())
}

fn bound_trade_cap(input: &mut trade::TradeInput, server_cap: &str) -> std::result::Result<(), String> {
    let server = crate::order_types::parse_u256(server_cap).map_err(|e| format!("{e}"))?;
    let client = input.max_amount.as_deref().map(crate::order_types::parse_u256).transpose().map_err(|e| format!("{e}"))?;
    input.max_amount = Some(client.map_or_else(|| server_cap.to_string(), |value| value.min(server).to_string()));
    Ok(())
}

#[derive(Debug, Deserialize, JsonSchema)]
struct BatchQuoteInput {
    chain: String,
    pairs: Vec<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{Address, B256, Signature};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingSigner {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl Signer for CountingSigner {
        fn address(&self) -> Address { Address::ZERO }

        async fn sign_message(&self, _: &[u8]) -> Result<Signature> {
            Err(eyre::eyre!("unexpected signature"))
        }

        async fn sign_hash(&self, _: B256) -> Result<Signature> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(eyre::eyre!("unexpected signature"))
        }
    }

    #[tokio::test]
    async fn malformed_mcp_trade_cap_rejects_before_signing() {
        // "" is the one that bites: from_str_radix("") is 0, so it used to become a zero cap only
        // after a quote and an RPC round trip. Every malformed shape must error before either.
        for cap in ["not-an-amount", "12abc", "", "  "] {
            let signer = Arc::new(CountingSigner { calls: AtomicUsize::new(0) });
            let server = AgentSwapMcp::new(Config {
                client: Client::new("http://127.0.0.1:1", None),
                intent_client: Client::new("http://127.0.0.1:1", None),
                signer: Some(signer.clone()),
                allow_trade: true,
                trade_max_amount: Some(cap.to_string()),
            });
            let result = server.trade(Parameters(trade::TradeInput {
                chain: "base".to_string(), from: "USDC".to_string(), to: "WETH".to_string(),
                amount: "1".to_string(), slippage: None, min_out: Some("1".to_string()),
                max_amount: None, mode: "agent-order".to_string(),
                proxy: "0x2222222222222222222222222222222222222222".to_string(),
                nonce: Some("1".to_string()), deadline_secs: Some(120), dry_run: true,
                self_submit: false,
            })).await;
            let error = match result {
                Ok(_) => panic!("malformed operator cap {cap:?} must fail"),
                Err(error) => error,
            };
            assert!(error.contains("invalid uint"), "cap {cap:?}: {error}");
            assert_eq!(signer.calls.load(Ordering::SeqCst), 0, "cap {cap:?} signed");
        }
    }
}
