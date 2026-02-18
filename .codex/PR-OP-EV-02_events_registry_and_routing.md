# PR-OP-EV-02: Events provider handler registry + native event routing (no NATS)

## Repo
`greentic-operator`

## Goal
Make operator demo capable of running `.gtpack` provider packs under:
- `providers/events/`

After `demo setup` + `demo up`, support triggering:
- webhook/SMS/email via HTTP ingress
- timer via scheduler

Providers emit events as CBOR; operator **routes events natively** into application pack flows (no NATS).

## Design

### 1) Discover events provider packs
Reuse the existing discovery pipeline; ensure demo includes:
- `providers/events/*.gtpack`
- CBOR-only and validation gate (later PR can make this “always on”)

### 2) Build handler registry
At `demo up` start:
- Load all configured provider packs (tenant/team)
- For each provider pack:
  - obtain handler declarations (short-term: from pack extension; medium-term: from component describe())
- Register:
  - HTTP handlers: `(domain=events, provider, handler_id)` -> `(component_ref, "ingest_http")`
  - Timer handlers: `(domain=events, provider, handler_id)` -> `(component_ref, op_id, schedule)`

### 3) Timer scheduler
Add a scheduler loop in `src/demo/runtime.rs`:
- For each configured timer handler:
  - create tokio task using interval only (seconds)
  - on tick, call provider op with CBOR input containing: handler_id, timestamp, tenant/team, last_run (optional)
  - decode output events list

### 4) Native event routing
Add an internal event router:
- For each emitted `EventEnvelopeV1`:
  - resolve destination pack using existing default hierarchy:
    - team-level default pack
    - tenant-level default pack
    - root-level `default.gtpack`
  - run selected pack default flow using the demo runner core.
- No static `events_routes` mapping is required in MVP.

### 5) Normalized event envelope
Require emitted events to use `EventEnvelopeV1` with at least:
- `event_id`
- `event_type`
- `occurred_at`
- `source` (`domain=events`, provider, handler_id)
- `scope` (tenant, optional team)
- `payload`
Optional:
- `correlation_id`
- `http`
- `raw`

## Implementation steps
1. Registry types
   - Add `src/demo/events_registry.rs`

2. Ingress dispatch integration
   - Extend PR-OP-EV-01 dispatch to accept events handlers (domain=events)

3. Timer scheduler
   - Add `spawn_events_timer_scheduler(...)` called by `demo_up`

4. Event router
   - Add `src/demo/event_router.rs`
   - Plumb emitted events from ingress + scheduler into the router

5. Minimal application pack binding
   - Implement default hierarchy pack resolution + default flow execution.

## Files to change/add
- Add: `src/demo/events_registry.rs`
- Add: `src/demo/event_router.rs`
- Update: `src/demo/runtime.rs`
- Update: `src/demo/ingress_dispatch.rs`

## Testing
- Unit tests: registry building from sample extension metadata
- Integration test: fake provider emits a test event, app pack flow runs.
- Unit tests: interval scheduler registration/execution (seconds-based) for timer handlers.

## Acceptance criteria
- Operator demo can run events provider packs placed under `providers/events/`
- HTTP ingress to `/v1/events/ingress/...` calls provider `ingest_http` and returns response
- Timer handlers trigger provider op on schedule and route events
- Events route into app packs via default hierarchy (team -> tenant -> root fallback)
- No NATS dependency required or legacy compatibility mode
