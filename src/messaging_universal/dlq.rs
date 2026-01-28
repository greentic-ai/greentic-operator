//! Dead-letter queue helpers for the universal pipeline.

use chrono::Utc;
use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Append a DLQ entry to the jsonl log.
pub fn append_dlq_entry(path: &Path, entry: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let serialized = serde_json::to_string(entry)?;
    writeln!(file, "{serialized}")?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn build_dlq_entry(
    job_id: &str,
    provider: &str,
    tenant: &str,
    team: Option<&str>,
    session_id: Option<&str>,
    correlation_id: Option<&str>,
    attempt: u32,
    max_attempts: u32,
    node_error: Value,
    message_summary: Value,
) -> Value {
    json!({
        "ts": Utc::now().to_rfc3339(),
        "job_id": job_id,
        "provider": provider,
        "tenant": tenant,
        "team": team,
        "session_id": session_id,
        "correlation_id": correlation_id,
        "attempt": attempt,
        "max_attempts": max_attempts,
        "node_error": node_error,
        "message_summary": message_summary,
    })
}
