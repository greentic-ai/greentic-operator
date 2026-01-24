OP-EVENTS-PR-02 — Start events services automatically when events providers exist
Goal

When providers/events/*.gtpack exists, dev up starts the events runtime services, exactly like messaging.

Key constraint

Don’t hardcode binaries in code. Services must be config-driven, but can have sensible defaults.

Config model (recommended)

In operator config:

services:
  messaging:
    enabled: auto
    components:
      - id: messaging-gateway
        binary: gsm-gateway
      - id: messaging-egress
        binary: gsm-egress

  events:
    enabled: auto
    components:
      - id: events-ingress
        binary: greentic-events-ingress
      - id: events-worker
        binary: greentic-events-worker


Rules:

If enabled:auto → start only if that domain has at least one provider pack

If enabled:true → always start

If enabled:false → never start

Implementation details

Extend existing service orchestration to support a second domain.

Ensure dev-mode binary resolution applies equally to events.

Ensure status/logging includes events services.

Tests

Fixture fake binaries for greentic-events-ingress, greentic-events-worker (same style as messaging fake services)

In temp project dir, create providers/events/x.gtpack

Run dev up --tenant t and assert:

events services pid/log files are created

messaging services are NOT started (unless messaging packs also present)

Acceptance criteria

✅ dev up starts events services when events packs exist

✅ dev up does not require --enable

✅ dev status clearly shows what was started and why (“auto enabled by providers/events/*.gtpack”)