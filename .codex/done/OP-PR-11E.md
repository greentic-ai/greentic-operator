# OP-PR-11E — Demo UX polish: pretty output, restart controls, and fast feedback

## Goal
Improve developer experience of `greentic-operator demo`:
- Clear, actionable output (URLs, endpoints, next commands)
- Granular restart semantics (`--restart gateway,egress,cloudflared,setup`)
- Improved `status` with grouping and hints
- `demo doctor` checks for missing binaries/config/ports before start

## CLI additions
- `greentic-operator demo up --restart <csv|all>`
- `greentic-operator demo doctor`
- Optional: `greentic-operator demo print-config` (resolved config with defaults)

## Output format
On `demo up`, print:
- Public URL
- Ingress endpoint URL(s)
- Services table (name, pid, log path, health)
- Providers configured + setup status
- “Try this” section (curl example; how to view logs)

On `demo status`, show:
- Running services
- Stopped services with last known pids/logs
- Key paths

## Implementation plan
### 1) Pretty printing helpers
- Use `comfy-table` or a minimal custom formatter
- Consider `--json` output mode for stable testing & scripting

### 2) Restart semantics
- Parse `--restart` into targets:
  - services: cloudflared, nats, gateway, egress, subscriptions
  - setup: providers
- Implement:
  - stop -> start for services
  - delete or rotate provider output files when restarting setup

### 3) Demo doctor
Checks:
- Config exists and parses
- Required binaries exist (resolver)
- Ports available (best-effort)
- cloudflared present if enabled
- Optional: warn if NATS disabled but services configured to use it

### 4) Tests
- Snapshot tests for text output OR prefer `--json` output snapshots
- Unit tests for restart parsing and doctor checks

## Files to add/change
- `src/pretty.rs`
- `src/cli/demo_doctor.rs`
- `src/cli/demo_print_config.rs` (optional)
- `src/cli/demo_up.rs` (restart)
- `tests/demo_doctor.rs`

## Acceptance criteria
- ✅ `demo up` output is clear and includes next steps
- ✅ `demo doctor` catches missing binaries before starting
- ✅ `--restart` works for a subset of services without stopping everything
