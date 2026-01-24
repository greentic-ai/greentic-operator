OP-PR-10 — Secrets passthrough to greentic-secrets (no reimplementation)
Status

Approved direction
Replaces / supersedes: ❌ SEC-PR-01 (cancelled)

Goal

Make greentic-operator a thin passthrough to greentic-secrets, preserving:

canonical multi-tenant / team / user secret handling

existing CLI UX and storage backends

existing validation and requirements logic

Operator must never store secret values or implement a secret backend.

Design principles

greentic-secrets remains the only authority for secrets

Operator only:

supplies tenant/team/env context

selects the correct provider pack

forwards commands

captures logs

No new types

No new interfaces

No duplication of logic

User-facing commands (operator)

Add a new command group:

greentic-operator dev secrets <SUBCOMMAND>

Subcommands (MVP)
init

Initialise secret requirements for a provider pack.

greentic-operator dev secrets init \
  --tenant <TENANT> \
  --team <TEAM> \
  --pack <PATH_TO_PROVIDER_PACK>


Operator behavior

Resolve tenant/team/env from operator context

Execute:

greentic-secrets init \
  --env <env> \
  --tenant <tenant> \
  --team <team> \
  --pack <provider-pack.gtpack> \
  --non-interactive


Stream stdout/stderr to console

Save logs to:

state/logs/secrets/init-<timestamp>.log

set | get | list | delete (optional but recommended)

All are pass-through wrappers.

Example:

greentic-operator dev secrets set TELEGRAM_BOT_TOKEN


Becomes:

greentic-secrets set TELEGRAM_BOT_TOKEN \
  --env <env> \
  --tenant <tenant> \
  --team <team>


Operator:

injects tenant/team defaults

preserves original exit codes

does not parse secret values

Integration with operator flows
During operator dev setup messaging

Recommended sequence:

Run provider pack requirements flow (if present)

Automatically call:

operator dev secrets init --pack <provider-pack>


Print clear next steps if secrets are missing:

Missing secrets for messaging.telegram.bot:
- TELEGRAM_BOT_TOKEN
Run: greentic-operator dev secrets set TELEGRAM_BOT_TOKEN


Operator does not infer secret values.

Binary discovery

Operator must locate the secrets binary safely:

Default: greentic-secrets on $PATH

Optional override:

--secrets-bin /path/to/greentic-secrets


Fail fast with a clear error if not found:

greentic-secrets binary not found. Install it or pass --secrets-bin.

What is explicitly NOT allowed in operator

❌ No secret storage
❌ No secret schemas
❌ No secret validation logic
❌ No duplication of greentic-secrets CLI behavior
❌ No “dev secret store” inside operator

Files to add / modify (operator)

src/cli/dev_secrets.rs

argument parsing

command forwarding

src/tools/secrets.rs

process spawning helper

log capture

src/cli/mod.rs

wire dev secrets subcommands

No changes to:

greentic-types

greentic-interfaces

greentic-secrets

Tests (lightweight)

Unit test:

command-line argv construction for init

Integration test (optional):

mocked greentic-secrets binary on PATH

assert correct arguments are passed

Acceptance criteria

greentic-operator never stores or reads secret values

greentic-secrets remains the sole secret authority

Operator setup flows work with real messaging providers

Removing operator does not break secrets

All existing repos remain unchanged

Codex prompt (ready to paste)
Implement OP-PR-10 in greentic-operator.

Add `greentic-operator dev secrets` as a passthrough to the greentic-secrets CLI.

Rules:
- Do NOT implement a secret store in operator.
- Do NOT add new types or interfaces.
- Operator only forwards commands with tenant/team/env defaults.
- Capture logs under state/logs/secrets/.
- Fail clearly if greentic-secrets binary is missing.

MVP subcommand:
- dev secrets init --pack <provider-pack>

Optional:
- set/get/list/delete passthroughs.

SEC-PR-01 is cancelled and must not be implemented.