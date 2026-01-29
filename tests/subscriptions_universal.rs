use anyhow::Result;
use chrono::Utc;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::tempdir;

use greentic_operator::demo::runner_host::{FlowOutcome, OperatorContext, RunnerExecutionMode};
use greentic_operator::messaging_universal::dto::{HttpInV1, HttpOutV1};
use greentic_operator::messaging_universal::ingress::build_ingress_request;
use greentic_operator::subscriptions_universal::{
    scheduler::Scheduler,
    service::{ProviderRunner, SubscriptionEnsureRequest, SubscriptionService},
    store::{AuthUserRefV1, SubscriptionState, SubscriptionStore},
};
use messaging_universal_dto::{
    SubscriptionDeleteInV1, SubscriptionEnsureInV1, SubscriptionRenewInV1,
};

#[derive(Clone)]
struct FakeRunner {
    response: serde_json::Value,
    calls: Arc<Mutex<Vec<String>>>,
}

impl FakeRunner {
    fn with_response(response: serde_json::Value) -> Self {
        Self {
            response,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl ProviderRunner for FakeRunner {
    fn invoke(
        &self,
        provider: &str,
        op: &str,
        _payload: &[u8],
        _context: &OperatorContext,
    ) -> Result<FlowOutcome> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}:{}", provider, op));
        Ok(FlowOutcome {
            success: true,
            output: Some(self.response.clone()),
            raw: None,
            error: None,
            mode: RunnerExecutionMode::Exec,
        })
    }
}

impl ProviderRunner for StubRunner {
    fn invoke(
        &self,
        provider: &str,
        op: &str,
        payload: &[u8],
        _context: &OperatorContext,
    ) -> Result<FlowOutcome> {
        match op {
            "subscription_ensure" => {
                let dto: SubscriptionEnsureInV1 = serde_json::from_slice(payload)?;
                self.record(op, serde_json::to_value(&dto)?);
                let expires = Utc::now().timestamp_millis() + 60_000;
                let subscription_id = format!(
                    "stub-{}-{}",
                    provider,
                    dto.binding_id.as_deref().unwrap_or("missing")
                );
                let output = json!({
                    "subscription": {
                        "subscription_id": subscription_id,
                        "expiration_unix_ms": expires,
                        "binding_id": dto.binding_id,
                    }
                });
                Ok(FlowOutcome {
                    success: true,
                    output: Some(output),
                    raw: None,
                    error: None,
                    mode: RunnerExecutionMode::Exec,
                })
            }
            "subscription_renew" => {
                let dto: SubscriptionRenewInV1 = serde_json::from_slice(payload)?;
                self.record(op, serde_json::to_value(&dto)?);
                let expires = Utc::now().timestamp_millis() + 120_000;
                let output = json!({
                    "subscription": {
                        "subscription_id": dto.subscription_id.clone(),
                        "expiration_unix_ms": expires,
                    }
                });
                Ok(FlowOutcome {
                    success: true,
                    output: Some(output),
                    raw: None,
                    error: None,
                    mode: RunnerExecutionMode::Exec,
                })
            }
            "subscription_delete" => {
                let dto: SubscriptionDeleteInV1 = serde_json::from_slice(payload)?;
                self.record(op, serde_json::to_value(&dto)?);
                Ok(FlowOutcome {
                    success: true,
                    output: None,
                    raw: None,
                    error: None,
                    mode: RunnerExecutionMode::Exec,
                })
            }
            "ingest_http" => {
                let dto: HttpInV1 = serde_json::from_slice(payload)?;
                self.record(op, serde_json::to_value(&dto)?);
                let response = HttpOutV1 {
                    v: 1,
                    status: 200,
                    headers: Vec::new(),
                    body_b64: Some("".to_string()),
                    events: Vec::new(),
                };
                let output = serde_json::to_value(&response)?;
                Ok(FlowOutcome {
                    success: true,
                    output: Some(output),
                    raw: None,
                    error: None,
                    mode: RunnerExecutionMode::Exec,
                })
            }
            other => Err(anyhow::anyhow!("unexpected op {other}")),
        }
    }
}

#[derive(Clone)]
struct RecordedCall {
    op: String,
    payload: Value,
}

#[derive(Clone)]
struct StubRunner {
    calls: Arc<Mutex<Vec<RecordedCall>>>,
}

impl StubRunner {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn record(&self, op: &str, payload: Value) {
        self.calls.lock().unwrap().push(RecordedCall {
            op: op.to_string(),
            payload,
        });
    }

    fn find_call(&self, op: &str) -> Option<RecordedCall> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|call| call.op == op)
            .cloned()
    }
}

#[test]
fn ensure_writes_binding_state() -> Result<()> {
    let temporary = tempdir()?;
    let runner = FakeRunner::with_response(json!({
        "subscription_id": "sub-123",
        "expiration_unix_ms": 1_700_000_000_000i64,
    }));
    let service = SubscriptionService::new(
        runner.clone(),
        OperatorContext {
            tenant: "demo".to_string(),
            team: Some("default".to_string()),
            correlation_id: None,
        },
    );

    let request = SubscriptionEnsureRequest {
        binding_id: "bind-abc".to_string(),
        resource: Some("/me/mailFolders('Inbox')/messages".to_string()),
        change_types: vec!["created".to_string()],
        notification_url: Some("https://example.com/webhook".to_string()),
        client_state: Some("state".to_string()),
        user: Some(AuthUserRefV1 {
            user_id: "test-user".to_string(),
            token_key: "token-key".to_string(),
            tenant_id: None,
            email: None,
            display_name: None,
        }),
        expiration_target_unix_ms: None,
    };

    let state = service.ensure_once("messaging.email", &request)?;
    assert_eq!(state.binding_id, "bind-abc");
    assert_eq!(state.subscription_id.as_deref(), Some("sub-123"));
    assert_eq!(state.user.as_ref().unwrap().user_id, "test-user");

    let store = SubscriptionStore::new(temporary.path());
    store.write_state(&state)?;
    let loaded = store.read_state(
        &state.provider,
        &state.tenant,
        state.team.as_deref(),
        &state.binding_id,
    )?;
    let loaded = loaded.expect("state file missing");
    assert_eq!(loaded.subscription_id.as_deref(), Some("sub-123"));
    assert_eq!(loaded.expiration_unix_ms, Some(1_700_000_000_000));
    Ok(())
}

#[test]
fn scheduler_renews_due_subscription() -> Result<()> {
    let temporary = tempdir()?;
    let store = SubscriptionStore::new(temporary.path());
    let state = SubscriptionState {
        binding_id: "bind-123".to_string(),
        provider: "messaging.email".to_string(),
        tenant: "demo".to_string(),
        team: Some("default".to_string()),
        resource: None,
        change_types: vec!["created".to_string()],
        notification_url: None,
        client_state: None,
        user: None,
        subscription_id: Some("orig".to_string()),
        expiration_unix_ms: Some(Utc::now().timestamp_millis() - 1_000),
        last_error: None,
    };
    store.write_state(&state)?;

    let runner = FakeRunner::with_response(json!({
        "subscription_id": "renewed",
        "expiration_unix_ms": Utc::now().timestamp_millis() + 60_000,
    }));
    let service = SubscriptionService::new(
        runner.clone(),
        OperatorContext {
            tenant: "demo".to_string(),
            team: Some("default".to_string()),
            correlation_id: None,
        },
    );
    let scheduler = Scheduler::new(service, store.clone());
    scheduler.renew_due(Duration::from_secs(0))?;

    let renewed = store
        .read_state("messaging.email", "demo", Some("default"), "bind-123")?
        .expect("state missing");
    assert_eq!(renewed.subscription_id.as_deref(), Some("renewed"));
    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|item| item.ends_with("subscription_renew"))
    );
    Ok(())
}

#[test]
fn scheduler_delete_removes_state() -> Result<()> {
    let temporary = tempdir()?;
    let store = SubscriptionStore::new(temporary.path());
    let state = SubscriptionState {
        binding_id: "bind-456".to_string(),
        provider: "messaging.email".to_string(),
        tenant: "demo".to_string(),
        team: Some("default".to_string()),
        resource: None,
        change_types: vec!["created".to_string()],
        notification_url: None,
        client_state: None,
        user: None,
        subscription_id: Some("to-delete".to_string()),
        expiration_unix_ms: Some(Utc::now().timestamp_millis() + 60_000),
        last_error: None,
    };
    store.write_state(&state)?;

    let runner = FakeRunner::with_response(json!({}));
    let service = SubscriptionService::new(
        runner.clone(),
        OperatorContext {
            tenant: "demo".to_string(),
            team: Some("default".to_string()),
            correlation_id: None,
        },
    );
    let scheduler = Scheduler::new(service, store.clone());
    scheduler.delete_binding(&state)?;

    let deleted = store.read_state("messaging.email", "demo", Some("default"), "bind-456")?;
    assert!(deleted.is_none());
    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|item| item.ends_with("subscription_delete")),
        "delete op not invoked"
    );
    Ok(())
}

#[test]
fn stub_runner_records_ensure_binding_and_user() -> Result<()> {
    let runner = StubRunner::new();
    let service = SubscriptionService::new(
        runner.clone(),
        OperatorContext {
            tenant: "demo".to_string(),
            team: Some("default".to_string()),
            correlation_id: None,
        },
    );
    let request = SubscriptionEnsureRequest {
        binding_id: "stub-bind".to_string(),
        resource: Some("/me/mailFolders('Inbox')/messages".to_string()),
        change_types: vec!["created".to_string()],
        notification_url: Some("https://example.com/webhook".to_string()),
        client_state: Some("state".to_string()),
        user: Some(AuthUserRefV1 {
            user_id: "alice@example.com".to_string(),
            token_key: "token-key".to_string(),
            tenant_id: None,
            email: None,
            display_name: None,
        }),
        expiration_target_unix_ms: Some(Utc::now().timestamp_millis() as u64 + 30_000),
    };
    let state = service.ensure_once("messaging.email", &request)?;
    assert!(
        state
            .subscription_id
            .unwrap_or_default()
            .starts_with("stub-")
    );
    let call = runner
        .find_call("subscription_ensure")
        .expect("ensure op not recorded");
    let dto: SubscriptionEnsureInV1 = serde_json::from_value(call.payload)?;
    assert_eq!(dto.binding_id.as_deref(), Some("stub-bind"));
    assert_eq!(dto.user.user_id, "alice@example.com");
    Ok(())
}

#[test]
fn stub_runner_sees_ingress_binding_id() -> Result<()> {
    let runner = StubRunner::new();
    let context = OperatorContext {
        tenant: "demo".to_string(),
        team: Some("default".to_string()),
        correlation_id: None,
    };
    let request = build_ingress_request(
        "email",
        None,
        "POST",
        "/ingress/email/subscriptions/stub-bind",
        Vec::new(),
        Vec::new(),
        b"{}",
        Some("stub-bind".to_string()),
        Some("demo".to_string()),
        Some("default".to_string()),
    );
    runner.invoke(
        "messaging.email",
        "ingest_http",
        &serde_json::to_vec(&request)?,
        &context,
    )?;
    let call = runner
        .find_call("ingest_http")
        .expect("ingest_http not recorded");
    let captured: HttpInV1 = serde_json::from_value(call.payload)?;
    assert_eq!(captured.binding_id.as_deref(), Some("stub-bind"));
    assert_eq!(captured.tenant_hint.as_deref(), Some("demo"));
    Ok(())
}
