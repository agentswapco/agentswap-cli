// Per-chain quote routing and normalization into the existing execution shape.
// BSC requires a simulated ERC-20 route; quote credentials stay on the original service.
use super::Client;
use eyre::{Result, eyre};
use serde_json::{Value, json};

pub(crate) fn uses_meta(chain: u64) -> bool {
    chain == 56
}

#[derive(Debug)]
pub(crate) struct NoRoute;

impl std::fmt::Display for NoRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no executable route")
    }
}
impl std::error::Error for NoRoute {}

pub(crate) fn is_no_route(error: &eyre::Report) -> bool {
    if error.downcast_ref::<NoRoute>().is_some() { return true; }
    let message = error.to_string().to_ascii_lowercase();
    message.starts_with("http 404 ") && (message.contains("no executable route") || message.contains("no route"))
}

impl Client {
    pub(super) async fn backend_quote(&self, body: &Value) -> Result<Value> {
        if !uses_meta(body["chain_id"].as_u64().unwrap_or_default()) {
            let mut body = body.clone();
            if let Some(object) = body.as_object_mut() { object.remove("taker"); }
            return self.post(crate::routes::QUOTE, &body).await;
        }
        let taker = body["taker"].as_str().ok_or_else(|| eyre!("quote requires the owner's V6 proxy as taker; pass --taker <0x> (MCP: taker)"))?;
        let proxy = crate::order_types::parse_address(taker)?;
        eyre::ensure!(!proxy.is_zero(), "quote requires a nonzero V6 proxy as taker");
        let request = json!({"chainId":body["chain_id"],"tokenIn":body["token_in"],
            "tokenOut":body["token_out"],"amountIn":body["amount_in"],"taker":taker,
            "slippageBps":body.get("slippage_bps").cloned().unwrap_or(json!(50))});
        #[cfg(not(test))]
        let url = format!("{}{}", crate::routes::meta_origin(), crate::routes::QUOTE);
        #[cfg(test)]
        let url = self.meta_quote_url.clone().unwrap_or_else(|| format!("{}{}", crate::routes::meta_origin(), crate::routes::QUOTE));
        let response = self.http.post(url).json(&request).send().await
            .map_err(|e| eyre!("request failed: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(eyre!("HTTP {status}: {body}"));
        }
        normalize(response.json().await.map_err(|e| eyre!("parse failed: {e}"))?)
    }
}

fn normalize(response: Value) -> Result<Value> {
    let best = &response["best"];
    if best.is_null() || best["simulation"]["status"] != "success" || best["value"] != "0" {
        return Err(NoRoute.into());
    }
    let output = required(best, "amountOut")?;
    let router = required(best, "target")?;
    let spender = match best.get("approveTarget") {
        None | Some(Value::Null) => router,
        Some(Value::String(value)) if value.is_empty() => router,
        _ => required(best, "approveTarget")?,
    };
    let data = required(best, "calldata")?;
    crate::order_types::parse_raw_amount("quote output", output)?;
    crate::order_types::parse_address(router)?;
    crate::order_types::parse_address(spender)?;
    eyre::ensure!(data.starts_with("0x") && data.len() > 2, "quote missing router calldata");
    hex::decode(&data[2..]).map_err(|e| eyre!("invalid router calldata: {e}"))?;
    Ok(json!({"output":output,"router":router,"route_path":best["provider"],
        "execution":{"target":router,"spender":spender,"calldata":data}}))
}

fn required<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name].as_str().ok_or_else(|| eyre!("quote response missing {name}"))
}
