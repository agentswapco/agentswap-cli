// Shared AgentSwap API route constants used by the standalone CLI.
// Exports: endpoint constants and path-formatting helpers.
// Deps: std formatting only.
pub const QUOTE: &str = "/quote";
pub const QUOTE_LOOKUP: &str = "/quote/:hash";
pub const HEALTH: &str = "/health";
pub const CHAINS: &str = "/api/chains";
pub const TOKENS: &str = "/api/tokens";
pub const STATUS: &str = "/api/status";

// Auth routes
pub const AUTH_CHALLENGE: &str = "/auth/challenge";
pub const AUTH_REGISTER: &str = "/auth/register";
pub const AUTH_KEY_INFO: &str = "/auth/key-info";
pub const AUTH_PRICING: &str = "/auth/pricing";
pub const AUTH_QUOTA_CLAIM: &str = "/auth/quota/claim";

// x402 payment protocol routes
pub const X402_VERIFY: &str = "/x402/verify";
pub const X402_SETTLE: &str = "/x402/settle";
pub const X402_DISCOVERY: &str = "/.well-known/x402";

/// Format pool inspection path: /api/pool/{chain_id}/{address}
pub fn pool(chain_id: u64, address: &str) -> String {
    format!("/api/pool/{chain_id}/{address}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_constants() {
        assert_eq!(QUOTE, "/quote");
        assert_eq!(QUOTE_LOOKUP, "/quote/:hash");
        assert_eq!(HEALTH, "/health");
        assert_eq!(CHAINS, "/api/chains");
        assert_eq!(TOKENS, "/api/tokens");
        assert_eq!(STATUS, "/api/status");
        assert_eq!(AUTH_CHALLENGE, "/auth/challenge");
        assert_eq!(AUTH_REGISTER, "/auth/register");
        assert_eq!(AUTH_KEY_INFO, "/auth/key-info");
        assert_eq!(AUTH_PRICING, "/auth/pricing");
        assert_eq!(AUTH_QUOTA_CLAIM, "/auth/quota/claim");
        assert_eq!(X402_VERIFY, "/x402/verify");
        assert_eq!(X402_SETTLE, "/x402/settle");
        assert_eq!(X402_DISCOVERY, "/.well-known/x402");
    }

    #[test]
    fn test_pool_path_formatting() {
        assert_eq!(pool(8453, "0x1234"), "/api/pool/8453/0x1234");
        assert_eq!(pool(1, "0xabcd"), "/api/pool/1/0xabcd");
        assert_eq!(pool(42161, "0xdeadbeef"), "/api/pool/42161/0xdeadbeef");
    }
}
