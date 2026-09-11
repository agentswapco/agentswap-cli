// Buy quota command for preparing quota purchase wallet calls.
// Exports: Args, run.
// Deps: crate::{client, tokens}, alloy::primitives::U256, eyre, serde_json.

use alloy::primitives::U256;
use eyre::{eyre, Result};

use crate::client::Client;
use crate::order_types::parse_raw_amount;
use crate::tokens::{chain_id_to_name, chain_name_to_id, format_amount, resolve_token, unknown_chain_id};

pub struct Args {
    pub chain_id: String,
    pub token: String,
    pub amount: String,
    pub json: bool,
}

pub async fn run(client: &Client, args: Args) -> Result<()> {
    // Quota purchase amounts retain their existing zero-accepted policy.
    parse_raw_amount("quota purchase amount", &args.amount)?;
    let chain_id = chain_name_to_id(&args.chain_id)
        .ok_or_else(|| eyre!("{}", unknown_chain_id(&args.chain_id)))?;
    let (quota_contract, usdc_address) = quota_config(chain_id)
        .ok_or_else(|| eyre!("quota purchase is only supported on Base and Arbitrum"))?;
    let (token_addr, token_sym, token_dec) = resolve_token(&args.token, chain_id)
        .ok_or_else(|| eyre!("unknown token '{}' on chain id {}", args.token, chain_id))?;
    let amount_in = args.amount.clone();

    if token_addr.eq_ignore_ascii_case(usdc_address) {
        let display = format_amount(&args.amount, token_dec);
        return print_usdc(chain_id, quota_contract, token_addr, token_sym, &display, &amount_in, args.json);
    }

    let resp = client
        .quote(&serde_json::json!({
            "chain_id": chain_id,
            "token_in": token_addr,
            "token_out": usdc_address,
            "amount_in": amount_in,
        }))
        .await?;
    let usdc_out = field(&resp, &["output"]).unwrap_or("0");
    let min_usdc_out = (parse_raw_amount("quota quote output", usdc_out)? * U256::from(95u64)) / U256::from(100u64);
    let estimated_quotes = parse_raw_amount("quota quote output", usdc_out)? / U256::from(100u64);
    let swap_calldata = field(&resp, &["execution", "calldata"])
        .or_else(|| field(&resp, &["calldata"]))
        .ok_or_else(|| eyre!("quote response missing swap calldata"))?;
    let swap_target = field(&resp, &["execution", "target"])
        .or_else(|| field(&resp, &["router"]))
        .unwrap_or("?");
    let swap_spender = field(&resp, &["execution", "spender"]).unwrap_or(swap_target);
    let output = serde_json::json!({
        "chain": chain_id_to_name(chain_id),
        "chain_id": chain_id,
        "quota_contract": quota_contract,
        "payment_token": {"address": token_addr, "symbol": token_sym, "decimals": token_dec},
        "amount": {"raw": amount_in, "display": format_amount(&args.amount, token_dec)},
        "estimated_usdc_out": usdc_out,
        "estimated_quotes": estimated_quotes.to_string(),
        "min_usdc_out": min_usdc_out.to_string(),
        "swap": {"spender": swap_spender, "target": swap_target, "calldata": swap_calldata},
        "steps": [
            {"to": token_addr, "function": "approve(address,uint256)", "args": [quota_contract, &amount_in]},
            {"to": quota_contract, "function": "buyWithToken(address,uint256,uint256,bytes)", "args": [token_addr, &amount_in, &min_usdc_out.to_string(), swap_calldata]}
        ]
    });

    if args.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    println!(
        "Estimated ~{} quotes for {} {} via buyWithToken, at 100 raw USDC units per quote (confirm the price with: agentswap pricing)",
        format_amount(&estimated_quotes.to_string(), 0),
        output["amount"]["display"].as_str().unwrap_or("?"),
        token_sym
    );
    println!("To: {quota_contract}");
    println!("Step 1: approve({quota_contract}, {amount_in}) on {token_addr}");
    println!("Step 2: buyWithToken({token_addr}, {amount_in}, {min_usdc_out}, {swap_calldata})");
    println!("Quoted USDC out: {usdc_out}");
    println!("Min USDC out (5% slippage): {min_usdc_out}");
    println!("Swap target: {swap_target}");
    println!("Swap spender: {swap_spender}");
    Ok(())
}

fn print_usdc(
    chain_id: u64,
    quota_contract: &str,
    token_addr: &str,
    token_sym: &str,
    amount_display: &str,
    amount_in: &str,
    json: bool,
) -> Result<()> {
    let quotes = parse_raw_amount("quota purchase amount", amount_in)? / U256::from(100u64);
    let output = usdc_purchase_json(
        chain_id, quota_contract, token_addr, token_sym, amount_display, amount_in, quotes,
    );
    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }
    println!(
        "Estimated {} quotes for {amount_in} ({amount_display} {token_sym}) via buyWithUSDC, at 100 raw USDC units per quote (confirm the price with: agentswap pricing)",
        format_amount(&quotes.to_string(), 0),
    );
    println!("To: {quota_contract}");
    println!("Call: buyWithUSDC({amount_in})");
    Ok(())
}

/// The USDC purchase call, as the JSON the command prints. `amount_in` is the caller's digits and
/// reaches `buyWithUSDC` unchanged; the display value is beside it, never in the call.
fn usdc_purchase_json(
    chain_id: u64,
    quota_contract: &str,
    token_addr: &str,
    token_sym: &str,
    amount_display: &str,
    amount_in: &str,
    quotes: U256,
) -> serde_json::Value {
    serde_json::json!({
        "chain": chain_id_to_name(chain_id),
        "chain_id": chain_id,
        "quota_contract": quota_contract,
        "payment_token": {"address": token_addr, "symbol": token_sym, "decimals": 6},
        "amount": {"raw": amount_in, "display": amount_display},
        "estimated_quotes": quotes.to_string(),
        "steps": [{"to": quota_contract, "function": "buyWithUSDC(uint256)", "args": [amount_in]}]
    })
}

fn quota_config(chain_id: u64) -> Option<(&'static str, &'static str)> {
    match chain_id {
        8453 => Some(("0xac78e3bf6e3ed0dc2ee830773a2e6f0b8b740967", "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913")),
        42161 => Some(("0xd0066bbc592ac748ee8734c9771c138130e6b703", "0xaf88d065e77c8cC2239327C5EDb3A432268e5831")),
        _ => None,
    }
}

fn field<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The client points at a closed port, so a rejection proves the amount was refused before the
    /// quote request went out. `1_000` is the shape the previous parser read as one thousand.
    #[tokio::test]
    async fn malformed_quota_amount_is_refused_before_the_quote_request() {
        for amount in ["", " ", "1_000", "1.5", "1e6", "raw:10000000", "-1", "0x10"] {
            let error = run(
                &Client::new("http://127.0.0.1:1", None),
                Args {
                    chain_id: "base".to_string(),
                    token: "USDC".to_string(),
                    amount: amount.to_string(),
                    json: true,
                },
            )
            .await
            .expect_err("malformed quota amount must fail");
            assert!(
                format!("{error}").contains("quota purchase amount"),
                "{amount}: {error}"
            );
        }
    }

    #[test]
    fn buy_with_usdc_is_called_with_the_digits_given() {
        let json = usdc_purchase_json(
            8453,
            "0xquota",
            "0xusdc",
            "USDC",
            "10.00",
            "10000000",
            U256::from(100_000u64),
        );
        assert_eq!(json["amount"]["raw"], "10000000");
        assert_eq!(json["steps"][0]["args"][0], "10000000");
        assert_eq!(json["steps"][0]["function"], "buyWithUSDC(uint256)");
        // The display value is present but is not what the call carries.
        assert_eq!(json["amount"]["display"], "10.00");
    }
}
