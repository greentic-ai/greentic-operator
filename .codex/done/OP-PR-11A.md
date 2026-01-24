# OP-PR-11A — Runtime State + Supervisor (process mgmt) foundation

## Goal
Introduce a robust, testable foundation for **starting/stopping local demo services**:
- A filesystem-backed runtime state directory
- A lightweight process supervisor (spawn, pidfile, log capture, health check)
- Baseline CLI subcommands: `demo status`, `demo logs`, `demo down` (even before other services are wired)

This PR intentionally **does not** integrate cloudflared nor start Greentic services yet; it creates the plumbing the next PRs rely on.

## User stories
- As a developer, I can run `greentic-operator demo status` and get a clear view of what’s running and where logs are.
- As a developer, I can run `greentic-operator demo down` and it reliably stops previously started services.
- As a developer, I can run `greentic-operator demo logs <service>` to tail or print logs.

## Design notes
- Use a per-demo runtime directory: `state/runtime/<tenant>.<team>/`
- Supervisor uses:
  - `state/pids/<tenant>.<team>/<service>.pid`
  - `state/logs/<tenant>.<team>/<service>.log`
  - `state/runtime/<tenant>.<team>/resolved/<service>.json` (resolved command + env + ports)
- All paths should be configurable via CLI flags (with defaults as above) for testability.

## CLI additions
### Commands
- `greentic-operator demo status [--tenant <t>] [--team <x>] [--state-dir <path>]`
- `greentic-operator demo logs <service> [--tail] [--tenant <t>] [--team <x>] [--state-dir <path>]`
- `greentic-operator demo down [--tenant <t>] [--team <x>] [--state-dir <path>] [--all]`

### Shared flags (demo subcommands)
- `--tenant` default: `demo`
- `--team` default: `default`
- `--state-dir` default: `./state`
- Optional: `--verbose`

## Implementation plan
### 1) Create a `runtime_state` module
- `RuntimePaths::new(state_dir, tenant, team)` producing:
  - `runtime_root()` => `state/runtime/<tenant>.<team>/`
  - `pids_dir()` => `state/pids/<tenant>.<team>/`
  - `logs_dir()` => `state/logs/<tenant>.<team>/`
  - `resolved_dir()` => `state/runtime/<tenant>.<team>/resolved/`
- Helpers:
  - `write_json(path, value)`
  - `read_json<T>(path) -> Option<T>`
  - `atomic_write(path, bytes)` (write temp + rename)

### 2) Create `supervisor` module
Core structs:
- `ServiceId(String)` (validated name)
- `ServiceSpec { id, argv: Vec<String>, cwd: Option<PathBuf>, env: BTreeMap<String,String> }`
- `ServiceHandle { id, pid: u32, started_at: DateTime, log_path: PathBuf }`
- `ServiceStatus { id, running: bool, pid: Option<u32>, log_path, last_error }`

Functions:
- `spawn_service(paths, spec) -> Result<ServiceHandle>`
  - Open/create log file (append)
  - Spawn child with stdout/stderr redirected to log
  - Write pidfile
  - Write resolved json (argv/env/cwd)
- `is_running(pid) -> bool` (cross-platform best effort)
- `stop_service(paths, id, graceful_timeout_ms) -> Result<()>`
  - Read pidfile, attempt graceful terminate, then kill
  - Remove pidfile
- `read_status(paths) -> Vec<ServiceStatus>`
  - For each pidfile, determine if running

Cross-platform approach:
- Prefer `sysinfo` for process existence checks.
- For termination: use `Child::kill()` as fallback; on Unix optionally send SIGTERM then SIGKILL.

### 3) Wire minimal CLI
- Use `clap` to add `demo` subcommand with `status|logs|down`.
- `demo logs`:
  - default prints last N lines (e.g. 200)
  - `--tail` streams (best-effort; can be unix-only initially with clear message)

### 4) Add integration-style tests (no external deps)
Best practice: create a Rust fixture binary `tests/fixtures/fake_service` that prints “ready” then sleeps.
Tests verify:
- pidfile created
- log file contains “ready”
- `demo status` reports running
- `demo down` stops it and removes pidfile

## Files to add/change
- `crates/greentic-operator/Cargo.toml` (deps: clap, serde, serde_json, anyhow, thiserror, chrono, sysinfo)
- `crates/greentic-operator/src/main.rs` (CLI)
- `crates/greentic-operator/src/cli/mod.rs`
- `crates/greentic-operator/src/runtime_state.rs`
- `crates/greentic-operator/src/supervisor.rs`
- `crates/greentic-operator/tests/supervisor_smoke.rs`
- `crates/greentic-operator/tests/fixtures/fake_service/` (tiny bin)

## Acceptance criteria
- ✅ `demo status` works with no services (prints “none running”)
- ✅ Supervisor can spawn a service and persist pid + logs
- ✅ `demo down` stops services started via supervisor
- ✅ Tests pass on Linux, macOS, Windows

## Follow-ups (next PRs)
- OP-PR-11B: Cloudflared management + public URL discovery
- OP-PR-11C: Start Greentic services (gsm-gateway/egress/subscriptions) with supervisor
- OP-PR-11D: Provider setup orchestration
