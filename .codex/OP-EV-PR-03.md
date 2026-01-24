OP-EVENTS-PR-03 — Auto “setup all providers” across domains
Goal

greentic-operator demo setup --tenant <t> runs setup for all discovered provider packs across both messaging and events—no explicit provider list needed.

Behavior

Enumerate all .gtpack in:

providers/messaging

providers/events

For each pack:

run setup_default via greentic-runner integration

persist:

state/runtime/<tenant>/providers/<provider_id>.setup.json

optionally verify_webhooks if flag set

Important

Keep it robust even if some packs don’t yet implement setup flows:

If flow missing: fail with a clear error including pack path and available flows (or mark skipped if you want “best effort”; I’d default to fail-fast for correctness).

Tests

Use fake runner fixture that:

records invoked pack path + flow

outputs JSON

Create temp project providers/events and providers/messaging with dummy pack files

Verify demo setup attempts both domains (in stable order) and writes outputs.

Acceptance criteria

✅ One command configures all detected providers

✅ Outputs persisted per provider

✅ Clear error when a provider pack is malformed