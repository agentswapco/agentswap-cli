// Flexible x402 response/request DTOs.
// Exports: PaymentRequired, Accept, PaymentPayload.
// Deps: serde for protocol JSON.

use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct PaymentRequired {
    #[serde(default)]
    pub accepts: Vec<Accept>,
}

impl PaymentRequired {
    pub fn parse(body: &str) -> Result<Self> {
        let value: serde_json::Value =
            serde_json::from_str(body).map_err(|e| eyre!("invalid x402 body: {e}: {body}"))?;
        if let Some(accepts) = value.get("accepts") {
            let accepts = serde_json::from_value(accepts.clone())
                .map_err(|e| eyre!("invalid x402 accepts: {e}"))?;
            return Ok(Self { accepts });
        }
        if let Some(accept) = value.get("accept") {
            let accept =
                serde_json::from_value(accept.clone()).map_err(|e| eyre!("invalid x402 accept: {e}"))?;
            return Ok(Self {
                accepts: vec![accept],
            });
        }
        Err(eyre!("x402 response has no accepts: {body}"))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accept {
    pub scheme: String,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub asset: Option<String>,
    #[serde(default)]
    pub pay_to: Option<String>,
    #[serde(default)]
    pub max_amount_required: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub scheme: String,
    pub network: String,
    pub payload: PaymentTransfer,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentTransfer {
    pub signature: String,
    pub authorization: TransferAuthorization,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferAuthorization {
    pub from: String,
    pub to: String,
    pub value: String,
    pub valid_after: String,
    pub valid_before: String,
    pub nonce: String,
}
