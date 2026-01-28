PR-OP-01: Vendor messaging-cardkit into greentic-operator
Goal

Bring the portable card downsampling/rendering engine into operator as an internal crate so operator can use it without depending on greentic-messaging or GSM code.

Non-goals

Do not import messaging-cardkit-bin.

Do not change operator runtime behavior yet (that’s PR-OP-02).

Do not change downsampling logic at all.

Files / Steps
1) Create crate in operator

Create directory: crates/messaging-cardkit/

Copy exactly from greentic-messaging:

crates/messaging-cardkit/Cargo.toml

crates/messaging-cardkit/src/**

crates/messaging-cardkit/tests/** (include golden tests if they exist)

crates/messaging-cardkit/tests/fixtures/** (if tests reference fixtures)

Any README.md, CHANGELOG.md (optional but recommended)

2) Add to operator workspace

In root Cargo.toml workspace:

add member: crates/messaging-cardkit

Ensure workspace dependency versions remain compatible (serde, schemars, etc.).

3) Ensure it builds cleanly

Run:

cargo check -p messaging-cardkit

cargo test -p messaging-cardkit

4) Dependency hygiene

Confirm messaging-cardkit in operator does not pull:

GSM gateway/egress crates

NATS

pack loader

dev-viewer

How:

cargo tree -p messaging-cardkit | rg -n "gsm|nats|gateway|egress|dev-viewer|pack_loader" should show nothing.

Acceptance criteria (DoD)

cargo test -p messaging-cardkit passes in greentic-operator

No new dependencies from GSM/NATS/dev-viewer are introduced

No behavior changes elsewhere (only workspace + crate addition)

Status protocol (for Codex)

STATUS: IN_FLIGHT (PR-OP-01) while working

STATUS: READY_FOR_REVIEW (PR-OP-01) when complete; stop and wait for “go”

PR-OP-02: Integrate cardkit into operator’s provider execution flow
Goal

Make operator runtime use messaging-cardkit to render/downsample cards as part of provider op execution, matching the audit: MessageCardEngine::render_card_snapshot runs inside operator/runtime before a platform renderer generates provider payload.

This PR wires cardkit into the operator flow so send/reply (and anywhere else operator renders cards) preserves current behavior even as operator is simplified.

Non-goals

Do not change provider pack content.

Do not modify gateway/egress code (those will be legacy/hidden; operator owns downsampling).

Do not reintroduce NATS/GSM architecture.

Where cardkit integrates in the operator flow (conceptual)

When a provider op is invoked (egress send/reply, ingress handle-webhook/ingest if they return cards, etc.), operator does:

Parse incoming schema-core message (bytes → structured message)

Detect card content (e.g., MessageCard / card snapshot present)

Render for target provider

CardKit::render(provider_type, card_json) (or IR-level API)

cardkit:

builds IR

downgrades based on tier vs provider target tier

emits provider payload + warnings

Embed rendered payload into the provider op request

i.e., transform universal outbound message into provider-ready fields

Execute the provider pack op (WASM) with the final JSON bytes

Return response

Key principle: Host remains generic. It only uses provider_type and schema-core message content; no per-provider branching outside cardkit.

Concrete wiring target (what to change)

You likely already have a “card engine” in operator today (from libs/core or similar). This PR replaces that dependency with the vendored messaging-cardkit.

1) Find the current operator-side card rendering entrypoint

Search in operator repo for:

MessageCardEngine

render_card_snapshot

PolicyDowngradeEngine

downgrade_for_platform

PlatformRenderer

Replace imports like:

use libs::core::messaging_card::...
with:

use messaging_cardkit::... (or whatever crate name you use)

2) Create one “CardRendering” module in operator runtime

Add a small wrapper module (operator-owned) to keep the rest of the runtime clean:

crates/operator-runtime/src/cards.rs (example)

Responsibilities:

expose a single function:

render_if_needed(provider_type, schema_core_message_bytes) -> (updated_message_bytes, warnings)

isolate any schema-core field probing here (where to find the card JSON in the message)

This way, operator’s op invocation code only calls render_if_needed(...).

3) Integrate into the provider invoke path

Locate the operator code that builds the payload passed into provider ops:

the place where outbound OutMessage becomes payload_bytes for invoke_provider_op

Add:

call render_if_needed(provider_type, payload_bytes) before invoking the provider pack.

This must run for:

send

reply

(For ingress ops, only do this if you render cards on ingress; many systems don’t.)

4) Maintain “capability profile source” strategy

Because you moved to cardkit, use the simplest profile source:

StaticProfiles inside cardkit for now (provider_type → target tier)
Later you can add pack-driven profiles again, but don’t block this PR on it.

5) Logs and trace

Add a single log line when rendering happens:

provider_type

original tier + target tier

downgraded boolean

number of warnings

No sensitive payload logging.

Tests
Unit tests

For each provider renderer (at least 2–3 to start), feed a known fixture card and assert:

downgraded matches expected (e.g., telegram basic + premium card → downgraded true)

warnings contain known keys

If you copied golden tests with PR-OP-01, reuse them.

Integration tests (operator runtime)

Add a small test that simulates:

building an outbound schema-core message containing a card

calling the operator “render_if_needed”

verifying updated payload is provider-specific

No WASM execution required for this PR.

Acceptance criteria (DoD)

Operator uses messaging-cardkit for card rendering/downsampling

No behavior regressions in existing card rendering (fixtures/goldens pass)

Provider invoke path for send/reply calls rendering step when card content is present

No GSM/NATS dependencies introduced

Status protocol (for Codex)

STATUS: IN_FLIGHT (PR-OP-02) while working

STATUS: READY_FOR_REVIEW (PR-OP-02) when complete; stop and wait for “go”

Optional PR-OP-02.1: Add operator demo command for rendering (no bin)

If you want a quick manual sanity check without importing messaging-cardkit-bin:

greentic-operator demo render-card --provider telegram --fixture <path>

reads JSON fixture

calls messaging-cardkit

prints RenderResponse JSON

This stays tiny and doesn’t bloat operator.