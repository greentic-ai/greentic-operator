# greentic-operator Demo Audit

## Scope + Evidence
- Audited demo command path, pack loading/validation, provider execution, QA/i18n, and ingress.
- Evidence from:
  - `Cargo.toml` `[dependencies]`
  - `src/cli.rs` (`DemoSubcommand`, `Demo*Args::run`, `run_domain_command`, `run_plan_item`, `start_demo_ingress_server`, `resolve_event_components`)
  - `src/demo/{runtime.rs,http_ingress.rs,runner_host.rs,runner.rs,build.rs,doctor.rs,pack_resolve.rs}`
  - `src/{domains/mod.rs,discovery.rs,provider_config_envelope.rs,component_qa_ops.rs,providers.rs,doctor.rs,config.rs}`

## 1) Demo Codepath Inventory

### Demo subcommands (from `src/cli.rs` `enum DemoSubcommand`)
- `demo build` -> `DemoBuildArgs::run` -> `demo::build_bundle`.
- `demo up` (hidden alias) / `demo start` -> `DemoUpArgs::run_start` -> `DemoUpArgs::run_with_shutdown` -> `demo::demo_up` (bundle mode) or `demo::demo_up_services` (config mode).
- `demo setup` -> `DemoSetupArgs::run` -> `run_domain_command` -> `run_plan` -> `run_plan_item`.
- `demo send` -> `DemoSendArgs::run` -> provider component ops (`render_plan`, `encode`, `send_payload`) through `DemoRunnerHost`.
- `demo receive` -> `DemoReceiveArgs::run` -> NATS subscribe + provider op (`handle-webhook`/`ingest`).
- `demo ingress` -> `DemoIngressArgs::run` -> synthetic HTTP ingress request -> provider `ingest_http`.
- `demo new` -> `DemoNewArgs::run` (bundle scaffold).
- `demo status` -> `DemoStatusArgs::run` -> `demo::demo_status_runtime`.
- `demo logs` -> `DemoLogsArgs::run` -> `demo::demo_logs_runtime`.
- `demo doctor` -> `DemoDoctorArgs::run` -> `demo::demo_doctor`.
- `demo allow` / `demo forbid` -> `DemoPolicyArgs::run` (gmap + sync + resolved manifest copy).
- `demo subscriptions ensure/status/renew/delete` -> `DemoSubscriptions*Args::run`.
- `demo run` -> `DemoRunArgs::run` interactive pack runner.
- `demo list-packs`, `demo list-flows`.

Note: there is no `demo diagnostics` subcommand; diagnostics exists on `dev diagnostics` path.

### Crate/runtime coupling per demo path
- Common operator crate deps: `greentic-runner-desktop`, `greentic-runner-host`, `greentic-types`, `tokio`, `hyper`, `async-nats`, `jsonschema` (`Cargo.toml`).
- No direct `greentic-messaging` or `greentic-events` Rust crate dependency in `Cargo.toml`.
- Runtime process dependencies still present:
  - Messaging legacy stack binaries (`gateway`, `egress`, `subscriptions-msgraph`) in `src/config.rs`.
  - Events binaries `greentic-events-ingress`, `greentic-events-worker` defaults (`src/config.rs` `default_events_components`), resolved/started by `resolve_event_components` (`src/cli.rs`) and `demo_up` (`src/demo/runtime.rs`).

### Where ingress is configured
- HTTP ingress server for demo start:
  - `start_demo_ingress_server` (`src/cli.rs`) binds to `demo_config.services.gateway.listen_addr:port`.
  - `HttpIngressServer::start` + request handling in `src/demo/http_ingress.rs`.
- Subject-based ingress receive path:
  - `ingress_subject_filter` and `run_demo_receive_async` in `src/cli.rs`.
  - Subscribes to `greentic.{domain}.ingress...` NATS subjects and dispatches provider ops.

## 2) Pack Loading + Validation Audit

### Loading pipeline (bundle + provider packs)
- Bundle build:
  - `demo::build_bundle` copies selected resolved manifests + packs/providers and writes `greentic.demo.yaml` (`src/demo/build.rs`).
- Discovery:
  - `discovery::discover_with_options` scans `providers/{messaging,events}` `.gtpack` (`src/discovery.rs`).
  - Demo paths often force CBOR-only via `DiscoveryOptions { cbor_only: true }`.
- Pack resolution:
  - `domains::discover_provider_packs_cbor_only` / `domains::discover_provider_packs` (`src/domains/mod.rs`).
  - `resolve_demo_provider_pack` filters by tenant/team resolved manifest allowlist (`src/cli.rs`).
- Plan + execution:
  - `domains::plan_runs` chooses `setup/diagnostics/verify` entry flows.
  - `run_plan_item` executes flow via `runner_integration` or `runner_exec`.

### Validation consistency
- Strong CBOR gate:
  - `domains::ensure_cbor_packs` called in `demo start`, `demo setup`, `demo send`, subscriptions ensure path.
- Doctor/validator gate is **not** consistent:
  - `demo build` can run `doctor` gate (`BuildOptions.run_doctor`).
  - `demo doctor` runs `demo::demo_doctor` but this helper does not pass validator packs and checks only `bundle/packs` (`src/demo/doctor.rs`).
  - `demo setup/send/receive/ingress/run/subscriptions` do not run doctor/validator packs automatically.

### Canonical CBOR provider config + provenance + drift
- Stored as canonical CBOR:
  - `provider_config_envelope::write_provider_config_envelope` writes `state/runtime/<tenant>/providers/<provider>/config.envelope.cbor` with `canonical::to_canonical_cbor`.
- Provenance fields present:
  - `resolved_digest` from pack bytes, `describe_hash`, optional `schema_hash`, `component_id`, `operation_id`.
  - Contract cache at `providers/_contracts/<resolved_digest>.contract.cbor`.
- Drift guardrail present:
  - `ensure_contract_compatible` compares stored vs resolved `describe_hash`; blocks unless `allow_contract_change`.
- Gap:
  - `describe_hash` is synthesized from manifest-derived placeholder `ComponentDescribe` with null IO schemas (not from runtime `describe` op), so drift signal is weaker than true self-described contract.

## 3) 0.6.0 Execution + Self-Describing Components

### What is good
- Provider component execution goes through `PackRuntime::load` + `invoke_provider` (`src/demo/runner_host.rs`), not `manifest.json`.
- Demo bundles enforce `manifest.cbor` in many paths (`ensure_cbor_packs`, cbor-only discovery).
- `primary_provider_type` uses `decode_pack_manifest(manifest.cbor)` + inline provider extension.

### Gaps
- Input/output ABI in operator codepath is JSON bytes (`serde_json::to_vec`) passed to component invocation (`run_provider_component_op*`, `component_qa_ops`); no explicit CBOR ABI contract enforcement in operator layer.
- No runtime `describe` retrieval/cache path found; schema/describe are inferred from manifest with placeholders in `provider_config_envelope.rs`.
- Non-demo discovery paths still accept `pack.manifest.json` fallback (`domains::read_pack_manifest_data`, `discovery::read_pack_id_from_manifest`), though demo paths usually set `cbor_only`.

## 4) QA + i18n Integration

### QA lifecycle modes
- Supported modes in code: `default/setup/upgrade/remove` (`component_qa_ops::QaMode`).
- Mode routing:
  - Setup flows in demo setup/provider setup force `QaMode::Setup`.
  - Other flows can map via `qa_mode_for_flow` from flow name.
- QA operation chain in `apply_answers_via_component_qa`:
  - `qa-spec` -> `i18n-keys` -> `apply-answers` + config schema validation.

### i18n routing
- i18n key existence validation is implemented (`validate_i18n_contract`).
- Missing: tenant/profile locale selection routing in operator.
  - No locale/profile parameter propagated via `OperatorContext`, `build_input_payload`, or demo CLI options.
  - No operator-side selection of localized prompt language; only key validation.

## 5) Ingress Unification (webhooks/SMS/email) Assessment

### Current ingress server
- `src/demo/http_ingress.rs` can parse domain-qualified path and dispatch provider `ingest_http` op.
- Route contract currently expects:
  - `/{domain}/ingress/{provider}/{tenant}/{team?}`
- It currently always builds messaging DTO (`messaging_universal::ingress::build_ingress_request`) and calls `run_ingress` in messaging module.

### Reuse potential
- Reusable parts:
  - HTTP listener/thread model (`HttpIngressServer`)
  - Tenant/team/provider extraction, headers/query/body capture, correlation-id plumbing.
- Needs refactor for generic ingress:
  - Replace hardcoded `messaging_universal::dto::HttpInV1/HttpOutV1` path with domain/provider-capability based ingress envelope.
  - Route dispatch should resolve op per declared handler in provider pack metadata, not hardcode `ingest_http`.

### Timer note
- Timer is non-HTTP. Use scheduler/runner loop:
  - Existing precedent: universal subscription scheduler loop in `src/demo/runtime.rs` (`spawn_universal_subscriptions_service` with periodic `renew_due`).
  - For event/timer packs, use periodic operator scheduler (cron/tokio interval) invoking timer ops directly, not HTTP ingress.

## Demo Readiness Matrix

| Area | Current | Expected for target state | Readiness |
|---|---|---|---|
| Messaging packs | Pack-based runtime invocation in operator; no `greentic-messaging` crate dep | Keep pack-driven, no legacy GSM runtime requirement | Partial (legacy GSM binaries still in start/services path) |
| Events packs | Events provider packs discoverable, but runtime still launches `greentic-events-*` binaries | Operator-run pack handlers without `greentic-events` runtime processes | Blocked |
| Ingress | HTTP ingress server exists, but messaging DTO/op assumptions remain | Domain-agnostic ingress handler registration from packs | Partial |
| Pack validation | CBOR-only checks present; doctor available but inconsistent; validator packs not universal | Enforced validation/validators on every consumed pack path | Partial |
| 0.6 self-describing | Component runtime invocation exists | Describe/schema from component contract as source of truth | Blocked |
| QA lifecycle | Setup/default/upgrade/remove modes and QA ops wiring exist | Same + flow/mode mapping policy clarity | Ready/Partial |
| i18n | i18n key validation exists via `i18n-keys` | Locale selection routed by tenant/profile in operator prompts | Blocked |

## Refactor Plan to Remove `greentic-events` Runtime Dependency

### Operator dependency surface (traits/interfaces)
- Introduce operator-local traits (conceptual modules):
  - `IngressAdapter`: parse HTTP request -> domain-agnostic ingress input bytes.
  - `ProviderOpResolver`: resolve pack/provider/operation from route + tenant/team.
  - `PackExecutor`: invoke provider op (`DemoRunnerHost` backend).
  - `IngressResultMapper`: map provider op output -> HTTP response + emitted events.
  - `TimerScheduler`: register and trigger non-HTTP handlers.
- Keep concrete execution on `greentic-runner-host::PackRuntime`; avoid domain service binaries.

### Pack-declared ingress handlers
- Require pack metadata/capability declaration for handlers, e.g.:
  - webhook/http handlers (`op_id`, method/path patterns, auth hints)
  - sms/email webhook handlers
  - timer handlers (`op_id`, schedule expression)
- Source of truth should be component describe/capability contract (0.6), not filename conventions.

### Runtime registration
- During `demo start`:
  - Discover packs -> resolve declared ingress/timer handlers -> register into in-memory router/scheduler.
  - HTTP requests dispatch by registration table to pack ops.
  - Timer registrations feed scheduler loop invoking provider ops.
- Keep existing NATS receive path as optional adapter, also backed by registration table (not hardcoded `handle-webhook`/`ingest` checks).

## Blocking Issues (with exact file paths)
- `src/config.rs`: default events runtime points to external binaries `greentic-events-ingress` / `greentic-events-worker`; keeps runtime dependency.
- `src/cli.rs`: `resolve_event_components` + `demo start` path still resolves/spawns event binaries.
- `src/demo/runtime.rs`: `demo_up` launches external `events_components`; not pack-invocation based.
- `src/demo/http_ingress.rs`: ingress path hard-wired to messaging DTO + `run_ingress` from `messaging_universal`.
- `src/messaging_universal/ingress.rs`: domain-specific ingress execution (`Domain::Messaging`, `ingest_http`), not generic events/webhook/SMS/email dispatch.
- `src/provider_config_envelope.rs`: provenance `describe_hash` synthesized from manifest placeholders; not fetched from component self-describing runtime contract.
- `src/component_qa_ops.rs`: validates i18n keys but has no locale selection input or tenant/profile locale routing.
- `src/cli.rs`: no tenant/profile locale option propagation in setup/send/ingress flows (`OperatorContext` has no locale).
- `src/demo/doctor.rs`: demo doctor omits validator-pack integration and only scans `bundle/packs`; inconsistent with full validation expectations.

