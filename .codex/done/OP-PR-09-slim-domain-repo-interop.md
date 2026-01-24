# OP-PR-09 — Operator-owned domain interop contracts (docs only, no types/interfaces changes)

Date: 2026-01-22

## Goal
Document the operator’s explicit expectations for messaging/events/secrets provider packs so domain repos can be slimmed safely.

This PR is **docs-only** inside greentic-operator:
- one short spec page per domain
- one generic spec page for the common lifecycle flows

## Deliverables
- `docs/domains/common.md`
  - required folder layout: `providers/<domain>/*.gtpack`
  - standard flows: setup_default, setup_custom (optional), diagnostics (recommended), verify_* (optional)
  - input payload conventions (tenant/team/public_base_url)
  - output expectations (RunResult artifacts stored in operator state)

- `docs/domains/messaging.md`
  - verify flow: verify_webhooks
  - optional flow: rotate_credentials
  - `public_base_url` recommended

- `docs/domains/events.md`
  - verify flow: verify_subscriptions (optional for now)
  - `public_base_url` recommended if webhooks are used

- `docs/domains/secrets.md`
  - validations primarily via doctor/validators
  - secrets requirements asset expectations

## Acceptance criteria
- Developers can follow docs to add a new provider pack and have operator handle it without touching types/interfaces.
