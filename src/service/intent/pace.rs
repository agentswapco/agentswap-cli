// Relay publish pacing for one owner's batch: a rolling-window admission before each intent is
// signed, and one resend of the identical signed body after an HTTP 429.
// Exports: Pacer, Clock. Deps: crate::client::{Client, RateLimited}, async-trait, tokio time.
use crate::client::{Client, RateLimited};
use eyre::Result;
use std::{collections::VecDeque, sync::{Arc, Mutex}};

/// Relay publishes admitted per rolling window, below the stream's per-owner limit.
const LIMIT: usize = 25;
const WINDOW_MS: u64 = 60_000;
/// Wait after a 429 when the relay names no delay, and the longest wait honoured.
const DEFAULT_RETRY_SECS: u64 = 60;
const MAX_RETRY_SECS: u64 = 120;

/// Monotonic time and sleeps; tests inject a virtual clock.
#[async_trait::async_trait]
pub(crate) trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
    async fn sleep_ms(&self, ms: u64);
}

struct SystemClock(tokio::time::Instant);

#[async_trait::async_trait]
impl Clock for SystemClock {
    fn now_ms(&self) -> u64 { u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX) }
    async fn sleep_ms(&self, ms: u64) { tokio::time::sleep(std::time::Duration::from_millis(ms)).await }
}

/// Counts every relay POST of one run, including a resend, in a rolling window.
pub struct Pacer {
    clock: Arc<dyn Clock>,
    sent: Mutex<VecDeque<u64>>,
}

impl Pacer {
    pub fn system() -> Self { Self::new(Arc::new(SystemClock(tokio::time::Instant::now()))) }

    pub(crate) fn new(clock: Arc<dyn Clock>) -> Self { Self { clock, sent: Mutex::new(VecDeque::new()) } }

    /// Waits until one more POST fits the window, then counts it.
    pub(crate) async fn admit(&self) {
        loop {
            let now = self.clock.now_ms();
            let wait = {
                let mut sent = self.sent.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                while sent.front().is_some_and(|t| t.saturating_add(WINDOW_MS) <= now) { sent.pop_front(); }
                match sent.front() {
                    Some(oldest) if sent.len() >= LIMIT => oldest.saturating_add(WINDOW_MS) - now,
                    _ => { sent.push_back(now); return; }
                }
            };
            self.clock.sleep_ms(wait).await;
        }
    }

    /// Posts a signed body admitted by `admit`. On a 429 it waits the relay's delay, then resends
    /// the same body once; a second 429 is returned as `RateLimited`. Other errors return as is.
    pub(crate) async fn publish(&self, relay: &Client, body: &serde_json::Value) -> Result<serde_json::Value> {
        let error = match relay.announce_intent(body).await {
            Ok(answer) => return Ok(answer),
            Err(error) => error,
        };
        let Some(limited) = error.downcast_ref::<RateLimited>() else { return Err(error) };
        self.clock.sleep_ms(retry_secs(limited.retry_after_secs) * 1000).await;
        self.admit().await;
        relay.announce_intent(body).await
    }
}

fn retry_secs(named: Option<u64>) -> u64 { named.unwrap_or(DEFAULT_RETRY_SECS).min(MAX_RETRY_SECS) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_defaults_and_caps() {
        assert_eq!((retry_secs(None), retry_secs(Some(7)), retry_secs(Some(500))), (60, 7, 120));
    }

    #[tokio::test]
    async fn system_clock_is_monotonic() {
        let clock = SystemClock(tokio::time::Instant::now());
        let before = clock.now_ms();
        clock.sleep_ms(2).await;
        assert!(clock.now_ms() >= before + 2);
    }
}
