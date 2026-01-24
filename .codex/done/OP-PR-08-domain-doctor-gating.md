# OP-PR-08 — Domain-aware doctor gating (validators-first demo reliability)

Date: 2026-01-22

## Goal
Add a **single validation gate** in greentic-operator for demos:
- Run `greentic-pack doctor` across relevant packs
- Use **local validator packs** when available
- Make it easy to validate **messaging**, **events**, and **secrets** independently or together

This replaces ad-hoc validation scattered across domain CLIs.

## New commands
### `greentic-operator dev doctor <domain|all>`
- `doctor messaging|events|secrets|all`
- Options:
  - `--tenant <TENANT>` (optional for validation; include if used in doctor context)
  - `--team <TEAM>` (optional)
  - `--strict` (fail on warnings if desired)
  - `--validator-pack <PATH>` repeatable override (advanced)

### `greentic-operator demo build` integration
- `demo build` must call `dev doctor all` before finalizing the bundle
- If doctor fails: demo build fails fast with clear error summary

## Pack selection rules
- Provider packs:
  - Validate all `.gtpack` under `providers/<domain>`
- Demo packs:
  - Validate all packs in `packs/` that are included in the resolved manifest for the tenant/team

## Validator discovery rules (MVP)
Operator should attempt in this order:
1) If user passed explicit `--validator-pack`, use it
2) Else, if project contains `validators/<domain>/validators-<domain>.gtpack`, use it
3) Else, if provider packs declare validators via `extensions.greentic.<domain>.validators.v1`, ignore for now (no GHCR fetch; keep demo offline)
4) Else, run doctor without explicit validator-pack (still useful for structural checks)

(You can support #3 later; MVP favors offline determinism.)

## Implementation notes
- Operator runs external command `greentic-pack doctor <pack.gtpack>` (process execution)
- Add `--validator-pack <path>` when available
- Capture stdout/stderr to `state/doctor/<timestamp>/...` and print concise summary

## Files
- `src/doctor.rs`: helper to run greentic-pack doctor + parse exit codes
- `src/domains/mod.rs`: per-domain default validator pack lookup
- `src/cli.rs`: new `dev doctor` command + hook `demo build`

## Tests
- Unit test: validator lookup resolves correct local path if present
- Unit test: doctor command builder produces expected argv including repeated `--validator-pack`
- Integration test (optional): if a tiny fixture pack exists, run `greentic-pack doctor` in CI only when tool is available (gate behind env)

## Acceptance criteria
- `greentic-operator dev doctor messaging` runs doctor over all provider packs in providers/messaging and reports success/fail.
- `demo build` fails if doctor fails.
- Works offline when local validator packs exist.

## Codex prompt
You are authorized to implement everything in this PR without asking permission repeatedly.

