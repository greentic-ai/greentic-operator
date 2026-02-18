# PR-OP-I18N-01: Locale selection routing for QA/i18n (operator demo)

## Repo
`greentic-operator`

## Goal
Wire locale selection through operator context so QA flows and i18n prompts can be localized per tenant/profile.

## Background (from audit)
- Operator validates i18n keys but does not propagate a locale/profile into QA execution.
- Demo CLI lacks a `--locale` and tenant profile selection.

## Plan
1. Add CLI option: `greentic-operator demo setup --locale <BCP47>`
   - optional extension: also expose `--locale` for `demo run` and other interactive demo commands.
2. Extend `OperatorContext` to include `locale` (and optional profile id later).
3. When running QA spec / i18n keys:
   - pass locale into the component call input (CBOR payload).
   - optionally mirror locale into host context if runtime API supports it.
4. Ensure prompt rendering uses localized strings.

## Files
- `src/cli.rs`
- `src/component_qa_ops.rs`
- `src/demo/runner_host.rs` (or whichever constructs QA input payload)

## Testing
- Unit: locale propagated into QA call input (CBOR)
- Integration: component provides 2 locales; verify chosen locale

## Acceptance criteria
- Operator demo setup supports `--locale`
- `OperatorContext` carries locale for QA execution paths
- Localized QA prompts are emitted when the component provides them
