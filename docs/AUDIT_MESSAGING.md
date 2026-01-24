# Messaging Audit (greentic-operator)

Scope: operator-side behavior for messaging provider packs and messaging runtime. No refactor; this is a descriptive audit based on current source.

## Pack discovery behavior (paths, globs, flags)
- Provider packs are discovered by scanning directories (no glob engine):
  - `providers/messaging/*.gtpack` and `providers/events/*.gtpack` for discovery (`src/domains/mod.rs`, `src/discovery.rs`).
  - The scan is a plain `read_dir` + `*.gtpack` extension check; non-`.gtpack` files are ignored.
- Resolved manifest generation uses `providers/<domain>` and `packs/` roots (project scan/resolve), writing `state/resolved/<tenant>[.<team>].yaml` (`src/project/scan.rs`, `src/project/resolve.rs`).
- Demo bundle setup uses `--bundle` and may further filter provider packs based on the resolved manifest for the tenant/team:
  - `resolved/<tenant>[.<team>].yaml` is read and `providers.<domain>` entries are used to restrict which `.gtpack` files are valid (`src/cli.rs`).
- Filtering flags:
  - `--domain` to scope discovery/planning to messaging/events/secrets.
  - `--provider` to match by pack_id, filename, filename stem, or substring (`src/domains/mod.rs`).
  - `--allow-missing-setup` and `--best-effort` to skip missing `setup_default` flows instead of failing fast (`src/domains/mod.rs`, `src/cli.rs`).

## Provider metadata consumption
- Pack manifest file: `pack.manifest.json` inside the `.gtpack` (zip) archive.
- Pack identity:
  - `meta.pack_id` preferred; otherwise `name` field; if neither, discovery falls back to filename stem (`src/domains/mod.rs`, `src/discovery.rs`).
- Entry flows:
  - `meta.entry_flows` if present; otherwise derived from `flows[*].id` and `flows[*].entrypoints` (`src/domains/mod.rs`).
  - If still empty, pack_id is used as a fallback entry flow (`src/domains/mod.rs`).
- Adapter mapping for messaging runtime:
  - `ResolvedManifest.providers["messaging"]` is read and each entry is converted to an absolute pack path.
  - `ResolvedManifest.packs` are also included in the `--pack` list (`src/services/messaging.rs`).

## Ops/flows assumed and payload/output shapes
- Flows assumed by operator:
  - `setup_default` (required by default for provider packs).
  - `verify_webhooks` (optional; invoked when `--verify-webhooks`).
  - `diagnostics` (only used when running diagnostics command).
- Flows *not* invoked by operator: `send`, `subscriptions` (operator never calls these directly).
- Input payload shape for setup/verify flows:
  - Base JSON: `{ "tenant": <tenant>, "team": <team>, "env": "dev" }`.
  - Optional `public_base_url` when cloudflared is enabled.
  - Optional override file merges JSON object or replaces payload entirely (`src/providers.rs`).
- Output handling:
  - Runner shell-out path writes `state/runtime/<tenant>/providers/<provider>.setup.json|verify.json|status.json` with stdout/stderr, parsed JSON (if stdout is JSON), and timestamps (`src/providers.rs`).
  - Embedded runner path writes `state/runs/<domain>/<pack>/<flow>/<timestamp>/` with `input.json`, `run.json`, `summary.txt`, and `artifacts_dir` pointer (`src/runner_exec.rs`, `src/state_layout.rs`).

## Invocation mechanics (runner shell-out vs embedded)
- Shell-out via runner binary (`greentic-runner` or `greentic-runner-cli`):
  - Used by `providers::run_provider_setup` and when `--runner-binary` is provided in demo setup (`src/providers.rs`, `src/cli.rs`, `src/runner_integration.rs`).
  - Command line uses `run --pack <pack> --flow <flow> --input <json>` (or runner-cli form with `--pack` + `--flow` + `--input`).
- Embedded runner (in-process):
  - Uses `greentic_runner_desktop::run_pack_with_options` when no runner binary is supplied (`src/runner_exec.rs`, `src/cli.rs`).

## Runtime services started for messaging
- Messaging runtime is started by invoking the `greentic-messaging` binary:
  - `greentic-messaging serve --tenant <tenant> [--team <team>] --no-default-packs --pack <pack>... [--packs-root <dir>] pack` (`src/services/messaging.rs`).
  - Env vars: `NATS_URL` (if present), `CARGO_TARGET_DIR` -> `<root>/state/cargo-target` (`src/services/messaging.rs`).
  - PID and logs are tracked under `state/` via `services::runner` (`src/services/messaging.rs`).
- NATS is started separately (optional) and is not a direct messaging dependency but is typically enabled when messaging providers exist (`src/cli.rs`, `src/services/nats.rs`).

## Dependency/coupling analysis (operator vs greentic-messaging)
- Cargo dependency: none. `cargo tree -i greentic-messaging` reports no package match; the operator does not link to a greentic-messaging crate.
- Runtime dependency: operator shells out to the `greentic-messaging` binary and assumes its CLI contract.
- Evidence (rg hits) show only string references to `greentic-messaging` and messaging pack paths (`docs/AUDIT_MESSAGING_EVIDENCE.md`).
- Operator *does* depend on the `greentic-runner-desktop` crate for embedded pack execution (`src/runner_exec.rs`).

## What we cannot delete from greentic-messaging yet
- The `greentic-messaging` binary must remain available on PATH (or configured via operator config) for dev/demo runtime.
- CLI surface required:
  - `serve` subcommand with `--tenant`, `--team` (optional), `--no-default-packs`, `--pack <path>...`, `--packs-root <dir>`, and trailing `pack` positional (`src/services/messaging.rs`).
- It must accept `.gtpack` files in the `--pack` list and read from `--packs-root`.
- It must work with `NATS_URL` env when provided.

## Optional debug command
Not implemented. If needed, a `greentic-operator debug messaging-audit` command could report detected messaging packs and planned flows/services without running any flows.
