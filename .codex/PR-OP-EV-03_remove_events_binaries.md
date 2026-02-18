# PR-OP-EV-03: Remove greentic-events runtime process dependency from demo

## Repo
`greentic-operator`

## Goal
Eliminate demo runtime spawning of external events binaries:
- `greentic-events-ingress`
- `greentic-events-worker`

Demo becomes pack-driven (provider packs under `providers/events/`) with:
- operator-owned HTTP ingress + timer scheduler
- provider component invocation via pack runtime
- no legacy NATS receive path/mode

## Background (from audit)
- `src/config.rs` defines default events components pointing to external binaries.
- `resolve_event_components` and `demo_up` still resolve/spawn those binaries.

## Plan
1. Remove default events binaries from `src/config.rs` defaults.
2. Remove `resolve_event_components` usage in `src/cli.rs` demo start path.
3. Update `src/demo/runtime.rs` to not spawn events services.
4. Ensure the functionality is covered by PR-OP-EV-02 (registry + scheduler).
5. Remove legacy NATS receive compatibility toggles from demo events path (no fallback mode).

## Files
- `src/config.rs`
- `src/cli.rs`
- `src/demo/runtime.rs`

## Testing
- `cargo test`
- Manual: `cargo run --bin greentic-operator -- demo start ...` verifies no attempt to spawn external binaries

## Acceptance criteria
- Demo start/up runs with events packs and does not reference greentic-events binaries
- No `greentic-events-ingress`/`greentic-events-worker` in demo paths
- No legacy NATS receive mode/flag required for events handling
