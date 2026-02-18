# PR-OP-01 — greentic-operator: component@0.6.0 QA-driven setup/upgrade/remove (CBOR + i18n)

Repo: `greentic-operator`

Note: supersedes older OP docs focused on renderer-library/cardkit. Those are historical/outdated.

## Goal
Make greentic-operator orchestrate provider/component lifecycle using **0.6.0 wizard ops**:
- `qa-spec(mode)` → greentic-qa → `apply-answers(mode, current_config, answers)`
- validate resulting config against `describe.config_schema`
- store configs as canonical CBOR per tenant/team/provider
- localize interactive prompts via greentic-i18n

## Decisions locked (2026-02-11)
- Target ABI: **greentic:component@0.6.0** world `component-v0-v6-v0`.
- Contract authority: **WASM `describe()`** is source of truth (operations + inline SchemaIR + config_schema).
- Validation: strict by default (no silent accept). Any escape hatches must be explicit flags.
- Encodings: CBOR everywhere; use canonical CBOR encoding for stable hashing and deterministic artifacts.
- Hashes:
  - `describe_hash = sha256(canonical_cbor(typed_describe))`
  - `schema_hash = sha256(canonical_cbor({input, output, config}))` recomputed from typed SchemaIR values.
- i18n: `component-i18n.i18n-keys()` required for 0.6.0 components; QA specs must reference only known keys.

## Decisions locked (2026-02-13)
- Active spec precedence: implement `.codex/PR-OP-01.md` and `.codex/PR-OP-02.md` as source-of-truth.
- Contract drift flag placement:
  - `--allow-contract-change` is available on all config-mutating commands (`default`, `setup`, `upgrade`, `remove`).
  - Default behavior is hard-fail when stored `describe_hash != resolved describe_hash`.
  - Diagnostic code: `OP_CONTRACT_DRIFT` (error severity).
  - Exit behavior: non-zero; classify as validation failure (not resolve failure).
- Persisted config/state shape:
  - Store one CBOR envelope per provider instance, not split files.
  - Path: `tenant/<t>/providers/<kind>/<id>/config.envelope.cbor` (or `config.cbor` containing envelope payload).
  - Envelope fields:
    - `config` (value or bytes)
    - `component_id`
    - `abi_version`
    - `resolved_digest`
    - `describe_hash`
    - `schema_hash` (recommended)
    - `operation_id` (usually `run` or config op name)
    - `updated_at` (optional; exclude from deterministic comparisons)
- i18n failure policy (all modes, including `remove`):
  - Missing `component-i18n.i18n-keys()` export is a hard failure (`OP_I18N_EXPORT_MISSING`).
  - QA spec referencing unknown i18n key is a hard failure (`OP_I18N_KEY_MISSING`).


## Scope
### In-scope
- Replace any manifest-driven config assumptions with WASM `describe()` + QA ops.
- Implement operator actions for modes:
  - default/setup/upgrade/remove mapped 1:1 to QaMode
- Persist config/state per tenant/team/provider as a canonical CBOR envelope.
- Validate configs against SchemaIR (strict).
- Interactive UI uses greentic-qa + greentic-i18n.

### Out-of-scope
- Pack creation (greentic-pack)
- Flow authoring (greentic-flow)

## Implementation tasks
1) Component resolution integration
- Operator receives resolved component artifact (via distribution client) with digest.
- Cache contract per digest (describe_hash + schemas).

2) QA orchestration
- For each action:
  - call `qa-spec(mode)`
  - render via greentic-qa, localized via greentic-i18n
  - read/write `answers/<mode>.answers.json` + `answers/<mode>.answers.cbor` (optional)
  - call `apply-answers(mode, current_config, answers_cbor)`
  - validate output config against config_schema

3) Config storage
- Store canonical CBOR config envelope in operator state store:
  - key format: `tenant/<t>/providers/<kind>/<id>/config.envelope.cbor`
- Keep config + provenance in one record (`component_id`, `abi_version`, `resolved_digest`, `describe_hash`, optional `schema_hash`, `operation_id`, optional `updated_at`).

4) Diagnostics
- Structured diagnostics with stable codes.
- Fail early on schema mismatch or missing exports.
- Include explicit codes for drift and i18n failures:
  - `OP_CONTRACT_DRIFT`
  - `OP_I18N_EXPORT_MISSING`
  - `OP_I18N_KEY_MISSING`

5) Tests
- Mock component runner (fixture outputs for describe/qa/apply).
- Tests for each mode and strict validation failures.
- Verify i18n hard-fail behavior for missing export and unknown keys, including `remove`.

## Acceptance criteria
- Operator can setup/upgrade/remove a 0.6.0 component using QA ops.
- Configs are validated and stored deterministically as single-file envelopes.
- i18n rendering works with locale fallback.
- `cargo test` passes.
