# PR-OP-EV-01: Generic demo HTTP ingress router (single server, multi-domain, v1 routes)

## Repo
`greentic-operator`

## Goal
Replace the messaging-specific demo ingress request path with a **domain-agnostic** HTTP ingress router so we can serve both domains from one server using a single versioned namespace.

…from **one** HTTP server (single bind address/port).

This PR does **not** add static route config. It makes ingress dispatch generic and convention-based.

## Background (from audit)
- `src/demo/http_ingress.rs` currently builds a messaging-specific DTO and routes through `messaging_universal::ingress::run_ingress`.
- We want a unified ingress that captures raw HTTP request data and dispatches to provider `ingest_http` using CBOR in/out.

## Design

### Route format
Use one versioned route convention:
```
/v1/{domain}/ingress/{provider}/{tenant}/{team?}/{handler?}
```
- `domain`: `messaging` or `events`
- `provider`: provider type/id
- `handler`: optional discriminator for providers with multiple ingress handlers

### Ingress input envelope (CBOR-only)
Introduce a neutral struct (in operator code) such as:
- method
- path
- query
- headers (multi)
- body bytes
- remote addr (optional)
- correlation id
- extracted: domain/provider/tenant/team/handler

Encoded as canonical CBOR before invoking provider op.

### Dispatch
- Resolve provider/handler by route convention and loaded provider packs.
- Invoke provider `ingest_http` via runner host / pack runtime:
  - `invoke_provider("ingest_http", input_cbor) -> output_cbor`

### Output mapping
Provider op returns CBOR describing:
- HTTP response status/headers/body and optionally emitted event envelopes.

For this PR, implement the HTTP response mapping and plumb emitted event envelopes through for downstream routing (implemented in EV-02).

## Implementation steps
1. Refactor ingress server
   - Update `src/demo/http_ingress.rs`:
     - parse domain from path
     - parse `/v1/...` route form
     - remove usage of messaging DTO builder
     - build CBOR ingress envelope
     - call a new `demo::ingress_dispatch::dispatch_http_ingress(...)`

2. Add ingress dispatch module
   - New module: `src/demo/ingress_dispatch.rs`
   - Responsibilities:
     - lookup handler from registry (initially: from pack extension metadata already loaded by demo)
     - invoke provider op
     - decode output to HTTP response struct

3. Introduce response struct
   - New struct: `IngressHttpResponse` (status, headers, body)
   - New struct: `IngressEmittedEvents` (opaque bytes or typed CBOR decode)

4. Wire into CLI
   - Ensure `start_demo_ingress_server` uses new dispatch path for both domains.
   - Keep one server/bind for both domains.

## Files to change
- `src/demo/http_ingress.rs`
- `src/demo/mod.rs`
- `src/demo/runner_host.rs` (if needed: helper to invoke op with CBOR)
- `src/cli.rs` (start_demo_ingress_server path parsing expectations)
- Add: `src/demo/ingress_dispatch.rs`
- Add: `src/demo/ingress_types.rs` (optional)

## Testing
- Unit tests for v1 path parsing and domain extraction.
- Unit test: registry lookup + dispatch chooses correct op_id.
- Integration test: start server on ephemeral port and POST to `/v1/messaging/ingress/dummy/...` using a stub runtime.

## Acceptance criteria
- One server handles both `/v1/messaging/...` and `/v1/events/...`
- No messaging DTO hardcoding remains in demo ingress codepath
- Ingress invocation is CBOR-first (`ingest_http` request/response)
