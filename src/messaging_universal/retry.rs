//! Retry scheduling helpers for the universal pipeline.

use chrono::Utc;
use greentic_types::ChannelMessageEnvelope;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay_ms: 500,
            max_delay_ms: 30_000,
            jitter_ms: 250,
        }
    }
}

impl RetryPolicy {
    pub fn backoff_ms(&self, attempt: u32) -> u64 {
        let shift = attempt.saturating_sub(1).min(63);
        let multiplier = 1u64 << shift;
        let base = self.base_delay_ms.saturating_mul(multiplier);
        std::cmp::min(base, self.max_delay_ms)
    }

    pub fn delay_with_jitter(&self, attempt: u32, jitter_ms: u64) -> Duration {
        let backoff = self.backoff_ms(attempt);
        let jitter = std::cmp::min(jitter_ms, self.jitter_ms);
        Duration::from_millis(backoff.saturating_add(jitter))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EgressJob {
    pub job_id: Uuid,
    pub provider: String,
    pub attempt: u32,
    pub max_attempts: u32,
    pub next_run_at_unix_ms: u64,
    pub envelope: ChannelMessageEnvelope,
    pub plan_cache: Option<Value>,
    pub last_error: Option<String>,
}

impl EgressJob {
    pub fn new(
        provider: impl Into<String>,
        envelope: ChannelMessageEnvelope,
        max_attempts: u32,
    ) -> Self {
        Self {
            job_id: Uuid::new_v4(),
            provider: provider.into(),
            attempt: 0,
            max_attempts: max_attempts.max(1),
            next_run_at_unix_ms: current_time_ms(),
            envelope,
            plan_cache: None,
            last_error: None,
        }
    }

    pub fn increment_attempt(&mut self) {
        self.attempt = self.attempt.saturating_add(1);
    }

    pub fn schedule_next(&mut self, delay_ms: u64) {
        self.next_run_at_unix_ms = current_time_ms().saturating_add(delay_ms);
    }

    pub fn with_plan(&mut self, plan: serde_json::Value) {
        self.plan_cache = Some(plan);
    }

    pub fn record_error(&mut self, error: String) {
        self.last_error = Some(error);
    }
}

fn current_time_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_steps_up_to_max() {
        let policy = RetryPolicy {
            base_delay_ms: 100,
            max_delay_ms: 400,
            ..Default::default()
        };
        assert_eq!(policy.backoff_ms(1), 100);
        assert_eq!(policy.backoff_ms(2), 200);
        assert_eq!(policy.backoff_ms(3), 400);
        assert_eq!(policy.backoff_ms(4), 400);
    }

    #[test]
    fn delay_injects_jitter() {
        let policy = RetryPolicy::default();
        let without_jitter = policy.delay_with_jitter(1, 0);
        let with_jitter = policy.delay_with_jitter(1, 123);
        assert!(with_jitter >= without_jitter);
        assert!(with_jitter <= without_jitter + Duration::from_millis(policy.jitter_ms));
    }
}
