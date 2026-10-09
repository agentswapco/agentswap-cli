// Sweep CLI output preserves every token row before reporting aggregate failure.
// Transaction failures use the same exit-status error type as trade.
use crate::{client::Client, service::{sweep, submit::Wait}, signer::Signer};
use eyre::Result;
use std::sync::Arc;

pub async fn run(client: &Client, signer: Arc<dyn Signer>, input: sweep::Input, allow: bool, cap: Option<&str>, json: bool) -> Result<()> {
    let output = sweep::sweep(client, signer, input, allow, cap, Wait::CLI).await?;
    if json { println!("{}", serde_json::to_string_pretty(&output)?); }
    else {
        for row in &output.tokens {
            println!("{}", serde_json::to_string(row)?);
        }
        if let Some(note) = &output.note { eprintln!("{note}"); }
    }
    output.check()
}
