# OP-PR-11D — Provider setup orchestration (setup_default + optional verify_webhooks)

## Goal
After services are started and public URL is known, orchestrate provider setup:
- Initialize secrets (`greentic-secrets init`) per tenant/team/provider
- Run provider pack setup flow `setup_default` using runner
- Optionally run provider verification flow `verify_webhooks`
- Persist setup output/state for subsequent `demo up` runs

Keeps the contract flexible while being practical for the demo.

## Inputs / outputs
### Inputs
- `public_base_url` (from cloudflared)
- `tenant`, `team`
- Provider list (`--providers msgraph,telegram`)

### Outputs (persisted)
- `state/runtime/<tenant>.<team>/providers/<provider>.setup.json`
- `state/runtime/<tenant>.<team>/providers/<provider>.verify.json`
- `state/runtime/<tenant>.<team>/providers/<provider>.status.json`

## CLI additions
- `greentic-operator demo up ... [--providers <csv>] [--skip-setup] [--verify-webhooks]`
- Optional: `greentic-operator demo setup [--providers <csv>] [--verify-webhooks]`

## Runner integration (MVP: shell-out)
Invoke runner via a wrapper (adjust args to actual runner CLI):
- `greentic-runner run --pack <provider-pack> --flow setup_default --input <json>`

Support:
- `--runner-binary <path>`
- Provider pack path from config (or default `demo/provider-packs/`)

### Setup payload (default)
Create a minimal default payload (can evolve later):
```json
{
  "tenant": "<tenant>",
  "team": "<team>",
  "public_base_url": "<public_base_url>",
  "env": "dev"
}
```
Allow `--setup-input <path.json>` to override/merge.

## Implementation plan
### 1) Provider selection + config
Extend config with providers, each with:
- pack path
- flow names (`setup_default`, `verify_webhooks`)
- any env vars

### 2) Secrets init
Shell out:
- `greentic-secrets init --tenant ... --team ...`
If not installed, fail with actionable message OR allow `--skip-secrets-init`.
Persist stdout/stderr summary.

### 3) Run setup flow
- Build input JSON (default + overrides)
- Invoke runner command
- Capture stdout/stderr and write to `providers/<provider>.setup.json`
- Parse JSON if possible; if not JSON, store raw output with metadata.

### 4) Optional verification
If `--verify-webhooks`, invoke `verify_webhooks` flow similarly and store output.

### 5) Idempotency
If setup output already exists:
- reuse unless `--restart setup` or `--force-setup`

## Tests
- Fixture `fake_runner` that prints JSON for setup/verify and exits 0
- Verify `demo up` triggers setup and writes provider output files

## Files to add/change
- `src/providers.rs`
- `src/runner_integration.rs`
- `src/cli/demo_up.rs` (call provider setup)
- `tests/provider_setup_smoke.rs`
- `tests/fixtures/fake_runner/`

## Acceptance criteria
- ✅ `demo up --providers msgraph` runs setup and persists outputs
- ✅ Re-running `demo up` reuses outputs unless forced
- ✅ `--verify-webhooks` runs verification and stores output
