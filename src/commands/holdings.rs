// CLI rendering for read-only portfolio and advisory grant links.
// Delegates all discovery, validation, valuation and URL construction to shared services.
use crate::{cli::Cli, client::Client, service::{portfolio, grant_link}};
use eyre::Result;

pub async fn portfolio(cli: &Cli, input: portfolio::Input) -> Result<()> {
    let key = cli.api_key.clone().filter(|key| !key.is_empty()).or_else(crate::credentials::load_api_key);
    let output = portfolio::portfolio(&Client::new(&cli.url, key), input).await?;
    if cli.json { println!("{}", serde_json::to_string_pretty(&output)?); }
    else {
        println!("Blocks {}..={}\n{}", output.from_block, output.to_block, output.warning);
        for row in output.tokens {
            println!("{} {} raw={} USD={} status={} dust={} sources={}", row.address, row.symbol,
                row.balance_raw.as_deref().unwrap_or("unknown"), row.value_usd.as_deref().unwrap_or("unpriced"),
                row.status, row.dust, row.sources.join(","));
        }
    }
    Ok(())
}

pub async fn grant_link(cli: &Cli, input: grant_link::Input) -> Result<()> {
    let output = grant_link::grant_link(input).await?;
    if cli.json { println!("{}", serde_json::to_string_pretty(&output)?); }
    else {
        println!("{}\n{}", output.url, output.warning);
        for token in output.tokens { println!("{} {} raw={} human={}", token.address, token.symbol, token.raw, token.human); }
        if let Some(policy) = output.replaced_policy { println!("Replaced policy: {}", serde_json::to_string(&policy)?); }
    }
    Ok(())
}
