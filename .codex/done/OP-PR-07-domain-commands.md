# OP-PR-07 — Domain-aware setup/diagnostics/verify commands (messaging/events/secrets)

Date: 2026-01-22

## Goal
Make **greentic-operator** explicitly domain-aware for **messaging**, **events**, and **secrets** for dev/demo, without building a generic plugin system.

This PR adds operator commands that:
- discover provider packs under `providers/<domain>/*.gtpack`
- run standard lifecycle flows using **greentic-runner-desktop as a library** (fast, no external binaries)
- store run artifacts in `state/` for demos

## User-visible UX (new commands)
### `greentic-operator dev setup <domain>`
Runs provider setup flows across all provider packs for the domain:
- messaging: `setup_default` (and optionally `setup_custom` later)
- events: `setup_default`
- secrets: `setup_default` (or skip if absent; see fallback rules)

### `greentic-operator dev diagnostics <domain>`
Runs `diagnostics` on each provider pack that defines it.

### `greentic-operator dev verify <domain>`
Runs domain-specific verify flows if present:
- messaging: `verify_webhooks`
- events: `verify_subscriptions` (skip if absent)
- secrets: none (verify via doctor in OP-PR-08)

All commands support:
- `--tenant <TENANT>` required
- `--team <TEAM>` optional
- `--provider <provider_type_or_pack_id>` optional filter (implementation choice; may filter by pack_id first for MVP)
- `--dry-run` prints plan only (packs + flows to run)
- `--parallel <N>` optional (default 1 for determinism in demos)

## Implementation details
### 1) Operator-side domain table (explicit, not plugin)
Create a small internal enum + config mapping:

- `Domain::Messaging`
- `Domain::Events`
- `Domain::Secrets`

With a mapping struct:
- providers_dir: `"providers/messaging" | "providers/events" | "providers/secrets"`
- setup_flow: `"setup_default"`
- diagnostics_flow: `"diagnostics"`
- verify_flows:
  - messaging: `["verify_webhooks"]`
  - events: `["verify_subscriptions"]`
  - secrets: `[]`

### 2) Provider pack discovery
- Scan `providers/<domain>` for `*.gtpack` files
- Ensure deterministic ordering (sort by filename)

### 3) Flow existence check (safe fallback)
To avoid hard failures if a pack does not contain a lifecycle flow:
- If `diagnostics` or `verify_*` is missing: **skip with a concise warning**
- For `setup_default`: **fail** if missing (unless `--allow-missing-setup` is provided)

Flow listing approach (choose one):
- Option A (recommended for speed/robustness): unzip `.gtpack` and parse `pack.manifest.json` to list `entry_flows` or flow descriptors.
- Option B: just call runner-desktop with `entry_flow=...` and interpret "flow not found" as skip (less clean logs).

Prefer **Option A** so operator can print a clear plan and avoid noisy runtime errors.

### 4) Execute flows via greentic-runner-desktop (library)
Add dependency on `greentic-runner-desktop` (path+version 0.4) and implement a helper:

`run_provider_pack_flow(pack_path, flow_id, tenant/team, input_json) -> RunResult`

Use:
- `RunOptions.entry_flow = Some(flow_id.to_string())`
- `RunOptions.input = input_json`
- `RunOptions.ctx.tenant_id/team_id/user_id` (user_id optional, default "operator")
- `RunOptions.dist_offline = true` by default for demos (override flag)
- store artifacts under project `state/runs/<domain>/<pack_id or filename>/<flow>/<timestamp>` (or let runner default `.greentic/runs` but copy summary into operator state)

### 5) Standard input payloads per domain (MVP)
Keep it minimal and stable:

- Common fields:
  - `tenant`
  - `team` (optional)
  - `operator_env` (optional)

- Messaging/events add:
  - `public_base_url` if operator has one (from OP-PR-04/05 tunnel config) else omit

Secrets: no special fields initially.

### 6) Output handling
For each run, operator writes:
- `state/runs/<domain>/<pack_label>/<flow>/run.json` (copy from runner output or save the returned RunResult)
- `state/runs/.../summary.txt` with Success/Failure and key fields
- `state/runs/.../artifacts_dir` link file (path to runner artifacts)

## Files / modules to add or modify
- `src/cli.rs` (or equivalent): add new subcommands under `dev`
- `src/domains/mod.rs`: domain mapping + provider pack discovery
- `src/runner_exec.rs`: wrapper around greentic-runner-desktop
- `src/state_layout.rs`: run output paths

## Tests
- Unit test: provider pack discovery returns deterministic list (create temp dir with fake files)
- Unit test: plan generation selects correct flows per domain
- Integration test (optional if fixtures available):
  - point at a small fixture provider pack `.gtpack` and ensure operator can run `diagnostics` with `--dry-run proved plan`

## Acceptance criteria
- `greentic-operator dev setup messaging --tenant acme` runs `setup_default` across all messaging provider packs found in `providers/messaging`.
- `diagnostics` and `verify` commands skip missing flows gracefully and report which packs ran.
- Results are written under `state/runs/...`.
- All existing operator tests pass.

## Codex prompt
You are authorized to implement everything in this PR without asking permission repeatedly.

