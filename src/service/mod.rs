// Shared service handlers used by CLI commands and MCP tools.
// Exports: quote, market, and trade service modules.
// Deps: crate::client plus feature-specific helpers.

pub mod market;
pub mod intent;
pub mod quote;
pub mod trade;
