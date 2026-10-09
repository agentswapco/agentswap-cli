// Shared service handlers used by CLI commands and MCP tools.
// Exports: quote, market, trade, intent, submit and token service modules.
// Deps: crate::client plus feature-specific helpers.

pub mod market;
pub mod portfolio;
pub mod grant_link;
pub mod intent;
pub mod quote;
pub mod submit;
#[cfg(test)]
mod test_rpc;
pub mod token;
pub mod trade;
