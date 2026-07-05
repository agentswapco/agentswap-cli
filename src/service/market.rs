// Read-only market service calls shared by CLI commands and MCP tools.
// Exports: tokens and pool lookup helpers.
// Deps: crate::{client, tokens}, eyre.

use crate::client::Client;
use crate::tokens::chain_name_to_id;
use eyre::{eyre, Result};

pub async fn tokens(client: &Client) -> Result<serde_json::Value> {
    client.tokens().await
}

pub async fn pool(client: &Client, chain: &str, address: &str) -> Result<serde_json::Value> {
    let chain_id = chain_name_to_id(chain).ok_or_else(|| eyre!("unknown chain: {chain}"))?;
    client.pool(chain_id, address).await
}
