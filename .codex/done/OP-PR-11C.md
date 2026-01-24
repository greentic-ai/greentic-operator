# OP-PR-11C — Start demo services (gsm-gateway, gsm-egress, subscriptions, optional NATS)

## Goal
Implement `greentic-operator demo up` to start the core local demo services using the supervisor:
- `gsm-gateway` (ingress)
- `gsm-egress`
- `gsm-msgraph-subscriptions` (MVP provider subscriptions)
- Optional: local NATS (or external URL)
- Persist resolved endpoints/ports into runtime state
- Provide clear `status` output with health checks

Assumes OP-PR-11A and OP-PR-11B are merged.

## Config (minimal schema)
Support `--config <path>` (default: `./demo/demo.yaml` if present, else `./greentic.operator.yaml`).

Example:
```yaml
tenant: demo
team: default

services:
  nats:
    enabled: true
    url: "nats://127.0.0.1:4222"
    spawn:
      enabled: true
      binary: "nats-server"
      args: ["-p", "4222", "-js"]

  gateway:
    binary: "gsm-gateway"
    listen_addr: "127.0.0.1"
    port: 8080

  egress:
    binary: "gsm-egress"

  subscriptions:
    msgraph:
      enabled: true
      binary: "gsm-msgraph-subscriptions"
      mode: "poll"   # demo MVP
```

## Binary resolution rules
Implement a resolver:
1) absolute path -> use if exists
2) else search:
   - `./bin/<name>`
   - `./target/debug/<name>` and `./target/release/<name>`
   - `$PATH`
If not found, fail with actionable error.

## Implementation plan
### 1) Add config loading + validation
- `Config` structs in `config.rs` using `serde_yaml`
- Validation: required fields, port ranges, and binary resolution.

### 2) Build service specs
`build_service_specs(config, public_base_url, nats_url) -> Vec<ServiceSpec>`
Include env vars:
- `GREENTIC_TENANT`, `GREENTIC_TEAM`
- `NATS_URL` (if enabled)
- `PUBLIC_BASE_URL` (from cloudflared)
- any service-specific env keys if required by the binaries

Persist `resolved/*.json` for each service.

### 3) Start sequence in `demo up`
1) Ensure runtime dirs exist
2) Start cloudflared (if enabled) -> public_base_url
3) Start NATS (if spawn enabled) else validate external URL
4) Start gateway (depends on PUBLIC_BASE_URL)
5) Start egress
6) Start subscriptions

Support `--restart <csv|all>`:
- stop selected services then start again
- reuse services not restarted

### 4) Health checks (MVP)
- Primary: process running
- Optional: log “ready” regex per service (configurable), or HTTP health checks if available.

### 5) Tests
- Unit tests: config parsing + validation
- Integration tests: fake service binaries that print “ready” and sleep, and (for gateway) bind to a port.

## Files to add/change
- `src/config.rs`
- `src/bin_resolver.rs`
- `src/cli/demo_init.rs` (template config writer, optional)
- `src/cli/demo_up.rs` (full sequencing)
- `tests/demo_up_smoke.rs`
- `tests/fixtures/fake_gsm_*` (tiny bins)

## Acceptance criteria
- ✅ `demo up` starts configured services and prints endpoints
- ✅ `demo status` reports them
- ✅ `demo down` stops them all
- ✅ Works without docker (pure local binaries)
