# PR-OP-01: Universal Ingress + Outbound Pipeline (with cardkit) + Retry/DLQ (greentic-operator)

**Repo:** greentic-operator  
**Status:** Ready for implementation (explicit steps)  
**Do not touch:** Microsoft Graph/Teams *subscription* renewal flows unless explicitly referenced by messaging inbound HTTP or outbound send/reply. If you see “subscription”, “Graph subscription”, “renew”, “expiration”, “notificationUrl”, “changeType”, “resource”, stop and leave it unchanged.

## 0) Outcome we want
Operator becomes the single messaging host with this pipeline:

**HTTP Ingress** → provider component op `ingest_http` → `greentic_types::ChannelMessageEnvelope` → app component op (existing) → outbound `ChannelMessageEnvelope` → **cardkit render pipeline** (`render_plan` and `encode`) → provider op `send_payload` → external API.

Reliability:
- `send_payload` returns `ok(...)` or `err(node-error)` via component@0.5.0.
- Operator implements **in-memory retry scheduler** using `node-error.retryable` + `node-error.backoff-ms` + attempt count from ctx.
- Failures that are permanent or exceed attempts go to **dlq.log**.

## 1) Canonical types & invocation contract (do not invent new names)
- Canonical message type: `greentic_types::ChannelMessageEnvelope`
- Invocation: `greentic:component@0.5.0` `node.invoke(ctx, op, input_json_string) -> ok(json_string) | err(node-error)`
- Retry signals: `node-error.retryable`, `node-error.backoff-ms`

## 2) Define the universal DTOs in operator (JSON, versioned)
Create a new module: `src/messaging_universal/mod.rs` with submodules:
- `dto.rs`
- `ingress.rs`
- `egress.rs`
- `retry.rs`
- `dlq.rs`
- `tests.rs` (or `tests/` integration tests)

### 2.1 DTOs (src/messaging_universal/dto.rs)
Define minimal JSON DTOs used between operator and provider components.
These are *operator-owned* DTOs (not in greentic-types yet). Keep them stable and versioned.

#### HttpInV1 (passed to provider `ingest_http`)
```jsonc
{
  "v": 1,
  "provider": "slack|telegram|teams|webex|whatsapp|webchat|email|dummy",
  "route": "events|interactive|webhook|default",   // operator hint, optional
  "binding_id": "optional-binding-id",             // for bound webhooks (telegram, etc)
  "tenant_hint": "optional-tenant",
  "team_hint": "optional-team",
  "method": "POST",
  "path": "/ingress/slack/events",
  "query": { "k": ["v"] },
  "headers": { "Header-Name": ["value"] },
  "body_b64": "base64-encoded-bytes"
}
```

#### HttpOutV1 (provider response for ingress)
```jsonc
{
  "v": 1,
  "status": 200,
  "headers": { "Content-Type": ["text/plain"] },
  "body_b64": "base64-encoded-bytes",
  "events": [ /* list of ChannelMessageEnvelope JSON objects */ ]
}
```

#### RenderPlanInV1 (operator -> provider `render_plan`)
```jsonc
{ "v": 1, "message": { /* ChannelMessageEnvelope */ } }
```

#### EncodeInV1 (operator -> provider `encode`)
```jsonc
{ "v": 1, "message": { /* ChannelMessageEnvelope */ }, "plan": { /* provider-common render-plan */ } }
```

#### ProviderPayloadV1 (provider output of `encode`)
Align to provider-common WIT `provider-payload` shape:
```jsonc
{
  "content_type": "application/json",
  "body_b64": "base64-encoded-bytes",
  "metadata_json": "{...optional json string...}"
}
```

#### SendPayloadInV1 (operator -> provider `send_payload`)
```jsonc
{
  "v": 1,
  "payload": { /* ProviderPayloadV1 */ },
  "tenant": { "tenant": "...", "team": "optional", "user": "optional", "correlation_id": "optional" },
  "reply_scope": { /* optional ReplyScope, if needed */ }
}
```

> NOTE: We are using base64 for binary because component@0.5.0 takes JSON string. Keep it consistent across all providers.

## 3) Implement HTTP ingress routing (src/messaging_universal/ingress.rs)
### 3.1 Routes
Implement these operator routes (exact):
- `/ingress/{provider}/...`  (provider may interpret remainder)
Examples that must work:
- `/ingress/slack/events`
- `/ingress/slack/interactive`
- `/ingress/telegram/{binding_id}`
- `/ingress/webchat/webhook`
- `/ingress/webex/webhook`
- `/ingress/teams/activities` (DO NOT touch Graph subscription flows; only raw inbound activity webhook)
- `/ingress/whatsapp/webhook`
- `/ingress/email/webhook` (if present; otherwise return 404 but keep op path reserved)

### 3.2 Ingress flow
For any request:
1) Build `HttpInV1` with `provider` and `route` derived from path.
2) Invoke provider component op **`ingest_http`**.
3) Parse `HttpOutV1`:
   - return `status/headers/body` to the HTTP client immediately
   - for each event in `events`: parse as `ChannelMessageEnvelope` and enqueue for app handling (in-process queue).

### 3.3 Provider component selection
Use existing operator logic for “resolve provider pack” (whatever is in `src/providers.rs` / discovery).
Add a single resolver function:
- `resolve_provider_component(provider: &str) -> {component_ref, export/world}`
This should resolve to the provider runtime component that supports `ingest_http/render_plan/encode/send_payload`.

If current discovery returns “export: handle-webhook”, treat that as legacy and **ignore** for operator runtime selection once new ops exist. (Providers will still expose `handle-webhook` temporarily during migration.)

## 4) App invocation (keep current, but define explicit interface)
Operator already has runner_exec/runner_integration code. Reuse it.
Define in `src/messaging_universal/egress.rs` a single function:
- `invoke_app(envelope: ChannelMessageEnvelope) -> Vec<ChannelMessageEnvelope>`

This should call the configured app gtpack/component op (whatever operator demo currently uses). Do not redesign app selection in this PR.

## 5) Outbound pipeline with cardkit (src/messaging_universal/egress.rs)
For each outbound `ChannelMessageEnvelope`:
1) Invoke provider op `render_plan` with `RenderPlanInV1`.
2) Invoke provider op `encode` with `EncodeInV1` (message + plan).
3) Invoke provider op `send_payload` with `SendPayloadInV1`.

### Where messaging-cardkit fits
Operator already contains messaging-cardkit. Use it as the orchestrator’s “policy glue” only if needed, but the **provider op `render_plan` is the authoritative plan**.
Concretely:
- Operator should not do provider-specific rendering.
- Operator may attach additional metadata to `ChannelMessageEnvelope` before render_plan (e.g., "preferred_format": "adaptive_card"), but the provider decides final format.
- If cardkit today produces RenderPlan locally, refactor it to call provider `render_plan` (do not duplicate logic).

## 6) Retry engine + DLQ (src/messaging_universal/retry.rs + dlq.rs)
### 6.1 Egress job model
Create `EgressJob`:
- `job_id` (uuid)
- `provider`
- `attempt` (u32)
- `max_attempts` (u32, default 5)
- `next_run_at_unix_ms` (u64)
- `envelope` (ChannelMessageEnvelope)
- `plan_cache` optional (RenderPlan JSON) — optional; simplest: recompute each attempt
- `last_error` optional

### 6.2 Scheduling policy (demo)
- If `node-error.retryable == true`:
  - delay = `node-error.backoff-ms` if present else exponential backoff:
    - base=500ms, cap=30s, jitter up to 250ms
  - next_run_at = now + delay
  - reschedule until `attempt == max_attempts`
- If `retryable == false`: DLQ immediately
- If attempts exceeded: DLQ

### 6.3 DLQ sink
Append JSON Lines to `dlq.log` in operator work dir:
Fields:
- ts, job_id, provider, tenant_id, team_id, session_id, correlation_id
- attempt/max_attempts
- node_error {code,message,retryable,backoff_ms,details}
- message_summary {id,text,channel,attachments_count}

Add `operator demo dlq-tail` command (optional) or just document `tail -f dlq.log`.

## 7) Tests (must be explicit)
### 7.1 Unit tests (operator)
Add tests for:
- `HttpInV1` serialization (base64 correctness)
- retry backoff calculation (deterministic with seeded rng)
- DLQ record write format

### 7.2 Integration tests (operator)
Re-use existing provider wasm host tests if present, otherwise add minimal:
- load a provider pack (dummy or webchat) and call:
  - ingest_http with a fixture payload that returns 1 event
  - render_plan -> encode -> send_payload using dummy HTTP client / recorded mock
- Ensure retries happen when provider returns `node-error {retryable:true}` (simulate failure then success on second attempt).

## 8) Explicit "do not touch" list
- Anything in operator related to:
  - Teams/Graph subscription renewal
  - “verify_webhooks” flows setup logic
  - provisioning or subscriptions (unless it’s just forwarding an inbound HTTP request to provider ingest_http)

## 9) Rollout
- Keep operator’s existing demo commands working.
- Add a new demo command: `greentic-operator demo ingress --provider <p> --path <...>` if helpful, but not required.

---

## Implementation checklist (quick)
- [ ] Add `src/messaging_universal/*`
- [ ] Add HTTP route handlers in existing router
- [ ] Implement provider invoke wrappers: `invoke_provider_op(provider, op, json_in) -> json_out`
- [ ] Implement outbound pipeline calling 3 ops
- [ ] Implement retry scheduler + dlq.log
- [ ] Add tests
- [ ] Update docs: operator is sole host; runner-host deprecated