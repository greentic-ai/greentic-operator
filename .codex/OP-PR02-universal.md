PR-OP-02: Universal Subscriptions Service (provider-op driven, delegated-ready) — GSM removed (greentic-operator)

Repo: greentic-operator
Status: Ready for implementation (explicit steps)
Goal: Operator provides subscriptions as a generic service by calling provider component subscription ops.
Key requirement: Must support delegated permissions (per-user) for Email (and potentially Teams).
Removal: Remove legacy GSM subscriptions binaries/embedding and config paths.

0) Outcome we want

Operator runs this lifecycle for any provider that supports subscriptions:

Desired subscriptions → provider op subscription_ensure → persist state → renew scheduler → provider op subscription_renew → delete on teardown via subscription_delete.

And inbound notifications flow:

Graph/Webhook → operator HTTP ingress → operator builds HttpInV1 including subscription binding → provider ingest_http enriches/normalizes → ChannelMessageEnvelope events.

Delegated email must work, meaning provider can identify which user/token to use.

1) Canonical subscription ops and DTO contract (do not invent other names)

Providers that support subscriptions must implement:

subscription_ensure

subscription_renew

subscription_delete

Operator calls these ops on provider components using JSON DTOs from messaging_universal_dto (shared crate).

2) DTO alignment: use messaging_universal_dto in operator (no mirroring)

Operator must depend on and use the exact structs from:

messaging_universal_dto

This avoids drift between operator and providers.

3) Extend universal DTOs to support delegated subscriptions (required for Email)

Update shared DTOs (in messaging_universal_dto) and consume them in operator.

3.1 Add AuthUserRefV1
{ "user_id": "your-user-id", "token_key": "secrets-key-for-refresh-token" }

3.2 Subscription Ensure / Renew / Delete DTOs carry user

Required for delegated:

SubscriptionEnsureInV1.user: AuthUserRefV1

SubscriptionRenewInV1.user: AuthUserRefV1

SubscriptionDeleteInV1.user: AuthUserRefV1

Operator must store and re-use the user reference.

4) Subscription binding: operator must pass binding info to ingest_http

To make delegated Email work, provider must be able to map inbound notifications to the right user/token.

4.1 Canonical rule

Every subscription created by operator must have a stable subscription binding id:

binding_id = stable ID operator generates (e.g. UUID)

operator persists binding_id -> subscription_state including user

4.2 Ingress rule (critical)

When operator receives a webhook notification that corresponds to a subscription, operator must set:

HttpInV1.binding_id = <binding_id for that subscription>

HttpInV1.tenant_hint/team_hint as usual

This is required so the provider (Email) can:

look up delegated user context (or be given enough hints)

fetch /me/messages/{id} for that specific user

If your HTTP ingress paths currently don’t include binding_id, add a route for subscription notifications:

/ingress/{provider}/subscriptions/{binding_id} (POST + GET for validation token)
and map it into HttpInV1.binding_id.

This avoids changing HttpInV1 again and keeps the user context resolvable.

5) Operator module: add src/subscriptions_universal/* (new service)

Create:

src/subscriptions_universal/mod.rs

desired.rs (compute desired subscriptions from config + tenant context)

service.rs (invoke provider subscription ops)

scheduler.rs (renew timing, retry/backoff)

store.rs (persist JSON state per binding_id)

tests.rs or integration tests under tests/

6) State store model (JSON, binding_id-first)

Persist per binding_id (not subscription_id) so inbound webhook routing is trivial:

state/subscriptions/{provider}/{tenant}/{team}/{binding_id}.json

Store:

binding_id

provider

tenant/team

user (AuthUserRefV1) required for delegated

resource

change_types

notification_url

client_state

subscription_id

expiration_unix_ms

last_error (optional)

attempt / next_run_at (optional)

7) Config: define desired subscriptions generically (including delegated user binding)

Operator needs config that can express “subscribe this user’s mailbox”.

Minimal structure:

subscriptions.enabled

subscriptions.webhook_base_url (public base URL)

subscriptions.desired[] entries with:

provider

resource

change_types

expiration_minutes

user (AuthUserRefV1) OR a reference to a user registry (for now keep it explicit)

Email delegated example:

provider: "email"

resource: "/me/mailFolders('Inbox')/messages"

change_types: ["created"]

user: { user_id, token_key }

8) Scheduler policy

Same style as messaging retry:

renew at expiration - skew (e.g. 10 minutes)

retryable errors get backoff, max attempts

permanent errors go to subscriptions.dlq.log

9) CLI commands (integrate into existing demo tree)

Add:

greentic-operator demo subscriptions ensure --provider <p> [--tenant ...] [--team ...] [--binding-id ...]

... status

... renew --all

... delete --all

... tail (optional)

10) Remove legacy GSM subscriptions (explicit deletion list)

Remove/disable:

dependency on gsm-subscriptions-teams

src/services/embedded.rs spawning gsm_subscriptions_teams::run_worker

config defaults that reference gsm-msgraph-subscriptions

fake binaries:

src/bin/fake_gsm_msgraph_subscriptions.rs

demo runtime wiring that starts subscriptions as GSM binary/service

docs referencing GSM subscriptions as the path forward

Replace with:

spawn_subscriptions_universal() which starts the new scheduler loop in-process.

11) Tests (must prove delegated email works structurally)

Add operator integration tests with a fake provider component that implements:

subscription_ensure/renew/delete

ingest_http that asserts binding_id is set for subscription routes

Test cases:

ensure creates state file keyed by binding_id containing user

inbound webhook to /ingress/email/subscriptions/{binding_id} produces HttpInV1.binding_id == binding_id

scheduler renews using stored user

delete removes state