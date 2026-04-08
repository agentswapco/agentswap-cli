// Buy quota command for preparing quota purchase wallet calls.
// Exports: Args, run.
// Deps: crate::{client, tokens}, alloy::primitives::U256, eyre, serde_json.

use alloy::primitives::U256;
use eyre::{eyre, Result};
use std::str::FromStr;

use crate::client::Client;
use crate::tokens::{chain_id_to_name, chain_name_to_id, format_amount, resolve_token, scale_amount};

pub struct Args {
    pub chain: String,
    pub token: String,
    pub amount: String,
    pub json: bool,
}

pub async fn run(client: &Client, args: Args) -> Result<()> {
    let chain_id = chain_name_to_id(&args.chain)
        .ok_or_else(|| eyre!("unknown chain: {}. Use: base, arbitrum", args.chain))?;
    let (quota_contract, usdc_address) = quota_config(chain_id)
        .ok_or_else(|| eyre!("quota purchase is only supported on Base and Arbitrum"))?;
    let (token_addr, token_sym, token_dec) = resolve_token(&args.token, chain_id)
        .ok_or_else(|| eyre!("unknown token '{}' on chain {}", args.token, args.chain))?;
    let amount_in = scale_amount(&args.amount, token_dec);

    if token_addr.eq_ignore_ascii_case(usdc_address) {
        return print_usdc(chain_id, quota_contract, token_addr, token_sym, &args.amount, &amount_in, args.json);
    }

    let resp = client
        .quote(&serde_json::json!({
            "chain_id": chain_id,
            "token_in": token_addr,
            "token_out": usdc_address,
            "amount_in": amount_in,
            "amount_usd": args.amount.parse::<f64>().unwrap_or(0.0),
        }))
        .await?;
    let usdc_out = field(&resp, &["output"]).unwrap_or("0");
    let min_usdc_out = (parse_u256(usdc_out, "quote output")? * U256::from(95u64)) / U256::from(100u64);
    let estimated_quotes = parse_u256(usdc_out, "quote output")? / U256::from(100u64);
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
        "amount": {"human": args.amount, "raw": amount_in},
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
        "Buy ~{} quotes for {} {} via buyWithToken",
        format_amount(&estimated_quotes.to_string(), 0),
        output["amount"]["human"].as_str().unwrap_or("?"),
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
    amount_human: &str,
    amount_in: &str,
    json: bool,
) -> Result<()> {
    let quotes = parse_u256(amount_in, "amount")? / U256::from(100u64);
    let output = serde_json::json!({
        "chain": chain_id_to_name(chain_id),
        "chain_id": chain_id,
        "quota_contract": quota_contract,
        "payment_token": {"address": token_addr, "symbol": token_sym, "decimals": 6},
        "amount": {"human": amount_human, "raw": amount_in},
        "estimated_quotes": quotes.to_string(),
        "steps": [{"to": quota_contract, "function": "buyWithUSDC(uint256)", "args": [amount_in]}]
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }
    println!(
        "Buy {} quotes for {} {} via buyWithUSDC",
        format_amount(&quotes.to_string(), 0),
        amount_human,
        token_sym
    );
    println!("To: {quota_contract}");
    println!("Call: buyWithUSDC({amount_in})");
    Ok(())
}

fn quota_config(chain_id: u64) -> Option<(&'static str, &'static str)> {
    match chain_id {
        8453 => Some(("0xac78e3bf6e3ed0dc2ee830773a2e6f0b8b740967", "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913")),
        42161 => Some(("0xd0066bbc592ac748ee8734c9771c138130e6b703", "0xaf88d065e77c8cC2239327C5EDb3A432268e5831")),
        _ => None,
    }
}

fn parse_u256(value: &str, label: &str) -> Result<U256> {
    U256::from_str(value).map_err(|_| eyre!("invalid {label}: {value}"))
}

fn field<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str()
}
