// Typed HTTP 429 answer from the AgentSwap API, carrying the server's retry delay.
// Exports: RateLimited, read from the `Retry-After` header first, then the body's `retryAfterSec`.
// Deps: reqwest, serde_json.

/// An HTTP 429 answer. Its text matches every other HTTP error: `HTTP 429 Too Many Requests: <body>`.
#[derive(Debug)]
pub(crate) struct RateLimited {
    /// Whole seconds from `Retry-After`, else from the body's `retryAfterSec`.
    pub retry_after_secs: Option<u64>,
    body: String,
}

impl RateLimited {
    pub(super) async fn read(resp: reqwest::Response) -> Self {
        let header = resp.headers().get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
        let body = resp.text().await.unwrap_or_default();
        Self::from_parts(header, body)
    }

    fn from_parts(header: Option<u64>, body: String) -> Self {
        let field = serde_json::from_str::<serde_json::Value>(&body).ok()
            .and_then(|v| v["retryAfterSec"].as_u64());
        Self { retry_after_secs: header.or(field), body }
    }
}

impl std::fmt::Display for RateLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP 429 Too Many Requests: {}", self.body)
    }
}

impl std::error::Error for RateLimited {}

#[cfg(test)]
mod tests {
    use super::RateLimited;

    #[test]
    fn header_wins_then_body_field_then_none() {
        let body = r#"{"error":"stream_rate_limited","retryAfterSec":9}"#;
        assert_eq!(RateLimited::from_parts(Some(7), body.into()).retry_after_secs, Some(7));
        assert_eq!(RateLimited::from_parts(None, body.into()).retry_after_secs, Some(9));
        assert_eq!(RateLimited::from_parts(None, "not json".into()).retry_after_secs, None);
        assert_eq!(RateLimited::from_parts(None, body.into()).to_string(), format!("HTTP 429 Too Many Requests: {body}"));
    }
}
