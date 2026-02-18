# PR-OP-02 — greentic-operator: provider pack integration + guardrails + offline E2E fixtures

Repo: `greentic-operator`

Note: supersedes older OP docs focused on renderer-library/cardkit. Those are historical/outdated.

## Goal
Complete operator integration for provider packs and ensure safe upgrades:
- Guardrails for contract drift (describe_hash changes)
- Offline E2E tests using fixture resolver (no network)
- Atomic writes/backup for config updates

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
- Drift guardrail behavior:
  - `--allow-contract-change` is available on all config-mutating commands (`default`, `setup`, `upgrade`, `remove`).
  - Default behavior is hard-fail when stored `describe_hash != resolved describe_hash`.
  - Diagnostic code: `OP_CONTRACT_DRIFT` (error severity).
  - Exit behavior: non-zero; classify as validation failure.
- Persisted record shape:
  - Store a single CBOR envelope per provider instance (`config.envelope.cbor` or `config.cbor` containing envelope payload).
  - Envelope fields include `config`, `component_id`, `abi_version`, `resolved_digest`, `describe_hash`, optional `schema_hash`, `operation_id`, and optional `updated_at` (excluded from deterministic comparisons).
- Atomic write and backup policy:
  - Atomic write is always enabled: write temp file, `fsync`, then rename.
  - Backup is only created when `--backup` is passed.
  - Backup path is fixed and overwritten each run: `config.envelope.cbor.bak`.
  - No timestamped backups in v1.
- Fixture compatibility target:
  - Match greentic-flow/pack fixture registry conventions exactly.
  - Mandatory layout:
    - `tests/fixtures/registry/index.json`
    - `components/<component_id>/describe.cbor`
    - `components/<component_id>/qa_default.cbor`
    - `components/<component_id>/qa_setup.cbor`
    - `components/<component_id>/qa_upgrade.cbor`
    - `components/<component_id>/qa_remove.cbor`
    - `components/<component_id>/apply_<mode>_config.cbor` (at least `setup`/`upgrade`/`remove`)
    - `components/<component_id>/i18n_keys.json` optional (if not derivable)
- i18n failure policy:
  - Missing `component-i18n.i18n-keys()` export is a hard failure in all modes (`OP_I18N_EXPORT_MISSING`).
  - QA spec references unknown i18n key are hard failures in all modes (`OP_I18N_KEY_MISSING`).


## Scope
### In-scope
- Drift guardrail:
  - if stored describe_hash differs from resolved describe_hash: fail unless `--allow-contract-change`
- Atomic update semantics:
  - write config envelope to temp, `fsync`, then rename; optional backup with `--backup`
- Offline fixtures:
  - `tests/fixtures/registry/` matches greentic-flow/pack fixture format exactly
  - E2E tests: setup → upgrade → remove

### Out-of-scope
- UI polish beyond basic prompts
- Distribution listing/search features

## Implementation tasks
1) Drift detection
- Store describe_hash with stored config.
- Compare at update time; block by default.
- Expose `--allow-contract-change` on all config-mutating commands.
- Emit `OP_CONTRACT_DRIFT` on mismatch and return non-zero validation failure.

2) Offline test harness
- Add fixture resolver for operator tests.
- Provide fixture components with QA specs and apply outputs using required registry layout and file names.

3) Atomic writes
- Implement `write_atomic(path, bytes, backup)` helper with temp-write, `fsync`, rename.
- If `--backup`, write/overwrite `config.envelope.cbor.bak`.
- Test behavior in temp dirs.

## Acceptance criteria
- Drift is detected and blocks by default.
- Drift override is available only via `--allow-contract-change`.
- Offline E2E tests run in CI without network.
- Atomic config updates implemented and tested.
- Backup behavior matches `--backup` + single `.bak` overwrite semantics.
- `cargo test` passes.
