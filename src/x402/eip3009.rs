// EIP-3009 TransferWithAuthorization signing for x402 exact payments.
// Exports: sign_payment.
// Deps: alloy sol-types, base64, crate::signer.

use crate::order_types::{parse_address, parse_raw_amount, parse_u256};
use crate::signer::Signer;
use crate::x402::types::{Accept, PaymentPayload, PaymentTransfer, TransferAuthorization};
use alloy::primitives::{Address, B256};
use alloy::sol;
use alloy::sol_types::{eip712_domain, SolStruct};
use base64::{engine::general_purpose::STANDARD, Engine};
use eyre::{eyre, Result};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

sol! {
    #[derive(Debug)]
    struct TransferWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }
}

pub async fn sign_payment(
    accept: &Accept,
    signer: Arc<dyn Signer>,
    chain_id: u64,
) -> Result<String> {
    let asset = accept
        .asset
        .as_deref()
        .ok_or_else(|| eyre!("selected x402 accept missing asset address"))?;
    let pay_to = accept
        .pay_to
        .as_deref()
        .ok_or_else(|| eyre!("selected x402 accept missing payTo"))?;
    let value = accept
        .max_amount_required
        .as_deref()
        .ok_or_else(|| eyre!("selected x402 accept missing maxAmountRequired"))?;
    // Fail closed: the EIP-712 domain below is hardcoded to Circle USDC
    // ("USD Coin"/"2"). Only sign if the asset is the known USDC address for
    // this chain, so the hardcoded domain cannot be applied to a foreign token.
    let expected_usdc = crate::tokens::resolve_token("USDC", chain_id)
        .map(|(addr, _, _)| addr)
        .ok_or_else(|| eyre!("no known USDC asset for chain {chain_id}; refusing to sign x402 payment"))?;
    if !asset.eq_ignore_ascii_case(expected_usdc) {
        return Err(eyre!(
            "x402 asset {asset} is not the known USDC address {expected_usdc} for chain {chain_id}; refusing to sign with the hardcoded USD Coin/2 EIP-712 domain"
        ));
    }
    let authorization = authorization(signer.address(), pay_to, value)?;
    let typed = typed_transfer(&authorization)?;
    let domain = eip712_domain! {
        name: "USD Coin",
        version: "2",
        chain_id: chain_id,
        verifying_contract: parse_address(asset)?,
    };
    let digest = typed.eip712_signing_hash(&domain);
    let signature = signer.sign_typed_hash(digest).await?;
    let payload = PaymentPayload {
        scheme: accept.scheme.clone(),
        network: accept.network.clone().unwrap_or_else(|| chain_id.to_string()),
        payload: PaymentTransfer {
            signature: format!("0x{}", hex::encode(signature.as_bytes())),
            authorization,
        },
    };
    let json = serde_json::to_vec(&payload)?;
    Ok(STANDARD.encode(json))
}

fn authorization(from: Address, pay_to: &str, value: &str) -> Result<TransferAuthorization> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| eyre!("clock before unix epoch: {e}"))?
        .as_secs();
    Ok(TransferAuthorization {
        from: format!("{from:?}"),
        to: pay_to.to_string(),
        value: value.to_string(),
        valid_after: "0".to_string(),
        valid_before: (now + 120).to_string(),
        nonce: nonce_hex()?,
    })
}

fn typed_transfer(auth: &TransferAuthorization) -> Result<TransferWithAuthorization> {
    Ok(TransferWithAuthorization {
        from: parse_address(&auth.from)?,
        to: parse_address(&auth.to)?,
        value: parse_raw_amount("x402 payment amount", &auth.value)?,
        validAfter: parse_u256(&auth.valid_after)?,
        validBefore: parse_u256(&auth.valid_before)?,
        nonce: parse_nonce(&auth.nonce)?,
    })
}

fn nonce_hex() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| eyre!("failed to generate x402 nonce: {e}"))?;
    Ok(format!("0x{}", hex::encode(bytes)))
}

fn parse_nonce(value: &str) -> Result<B256> {
    let trimmed = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(trimmed).map_err(|e| eyre!("invalid nonce hex: {e}"))?;
    if bytes.len() != 32 {
        return Err(eyre!("x402 nonce must be 32 bytes"));
    }
    Ok(B256::from_slice(&bytes))
}
