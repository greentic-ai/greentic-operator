# greentic-operator

Greentic Operator orchestrates a project directory for demos and local development.
It manages tenants/teams, access mapping (.gmap), pack/provider discovery, resolved manifests, and starting local runtime services.

## Quickstart (dev)

```bash
mkdir my-demo && cd my-demo
greentic-operator dev init
greentic-operator dev tenant add tenant1
greentic-operator dev team add --tenant tenant1 team1
# drop provider packs into providers/messaging/
# drop packs into packs/
greentic-operator dev sync
greentic-operator dev up --tenant tenant1 --team team1
greentic-operator dev logs
```

Access mapping (.gmap)

Rules are line-oriented:

<path> = <policy>

Paths:

_ default

pack_id, pack_id/_, pack_id/flow_id, pack_id/flow_id/node_id

Policies (MVP):

public

forbidden

Team rules override tenant rules.

Demo bundles

greentic-operator demo build --out demo-bundle --tenant tenant1 --team team1
greentic-operator demo up --bundle demo-bundle --tenant tenant1 --team team1
Note: demo bundles require CBOR-only packs (`manifest.cbor`). Rebuild packs with `greentic-pack build` (avoid `--dev`).

### allow/forbid commands

There are two sets of gmap editing helpers:

- `greentic-operator dev allow/forbid` edits the project under the current working directory. The `--path` argument uses the classic `PACK[/FLOW[/NODE]]` segments and applies the policy inside `tenants/<tenant>[/teams/<team>]/(tenant|team).gmap`, followed by `dev sync`.
- `greentic-operator demo allow/forbid` is meant for portable bundles. Supply `--bundle <DIR>` plus `--tenant`/`--team` and pass the same `PACK[/FLOW[/NODE]]` path. The command rewrites the bundle’s gmap, reruns the resolver, and copies the updated `state/resolved/<tenant>[.<team>].yaml` into `resolved/`, so `demo up` immediately sees the change.

Paths must contain at most three segments. Passing `PACK/FLOW/NODE/EXTRA` (or relative paths with more than three parts) will trigger the “too many segments” error you saw. Stick to the `pack`, `pack/flow`, or `pack/flow/node` forms.

Demo send (generic)

greentic-operator demo send --bundle demo-bundle --provider telegram --print-required-args
greentic-operator demo send --bundle demo-bundle --provider telegram --text "hi" --arg chat_id=123

Demo new (bundle scaffold)

greentic-operator demo new demo-bundle
greentic-operator demo new demo-bundle --out /tmp

Creates the directory layout plus minimal metadata (`greentic.demo.yaml`, `tenants/default/tenant.gmap`, `providers/*`, `state`, `resolved`, `logs`, etc.) so you can start adding packs and tenant definitions before running `demo setup`/`demo build`.

Demo receive (incoming)

Terminal A: `greentic-operator demo receive --bundle demo-bundle`
Terminal B: `greentic-operator demo send --bundle demo-bundle --provider telegram --text "hi" --arg chat_id=123`

`demo receive` listens for the bundle's messaging ingress subjects, streams each message to stdout, and appends a JSON line to `incoming.log`. Use `--provider` to focus on a single provider or `--all`/default to watch every enabled messaging pack.

## Domain auto-discovery

Domains are enabled automatically when provider packs exist:

- messaging: `providers/messaging/*.gtpack`
- events: `providers/events/*.gtpack`

You can override per-domain behavior in `greentic.yaml`:

```yaml
services:
  messaging:
    enabled: auto   # auto|true|false
  events:
    enabled: auto   # auto|true|false
```

## Dev/demo dependency mode

Dev/demo uses local path dependencies for greentic-* crates with `version = "0.4"` and
`path = "../<repo>"`. Publishing (future) requires stripping path deps and relying on
registry-only versions.

## Local dev binaries

When iterating in a workspace/monorepo, you can resolve binaries from local build outputs
instead of relying on `cargo binstall` or `$PATH`:

```bash
greentic-operator dev on --root /projects/ai/greentic-ai --profile debug
greentic-operator dev detect --root /projects/ai/greentic-ai --profile debug --dry-run
greentic-operator demo up
greentic-operator demo doctor
```

Config (greentic.yaml) supports dev mode defaults and explicit binary overrides:

```yaml
dev:
  mode: auto
  root: /projects/ai/greentic-ai
  profile: debug
  target_dir: null
  repo_map:
    greentic-pack: greentic-pack
    greentic-secrets: greentic-secrets
    gsm-gateway: gsm-gateway
binaries:
  greentic-pack: /custom/bin/greentic-pack
```

Resolution order (hybrid dev-mode):
1) Explicit config path (binaries map or command path).
2) Dev-mode repo_map override under `dev.root` (if enabled).
3) Fallbacks (`./bin`, `./target/*`, then `$PATH`).

Global dev-mode settings are stored in `~/.config/greentic/operator/settings.yaml` (platform-
appropriate equivalents on macOS/Windows). Use `greentic-operator dev status` to view them and
`greentic-operator dev off` to disable dev mode globally.

## Demo service config (gsm-* services)

You can run demo services from a local config file instead of a bundle:

```yaml
tenant: demo
team: default
services:
  nats:
    enabled: true
    url: "nats://127.0.0.1:4222"
    spawn:
      enabled: true
      binary: "nats-server"
      args: ["-p", "4222", "-js"]
  gateway:
    binary: "gsm-gateway"
    listen_addr: "127.0.0.1"
    port: 8080
  egress:
    binary: "gsm-egress"
  subscriptions:
    msgraph:
      enabled: true
      binary: "gsm-msgraph-subscriptions"
      mode: "poll"
```

Run with:

```bash
greentic-operator demo up --config ./demo/demo.yaml
```

## Embedded GSM services (WIP)

The operator now contains an `services::embedded` helper that runs `gsm-gateway`, `gsm-egress`, and
`gsm-subscriptions-teams` inside the operator process using their existing async runtimes. This prevents the demo/dev
workflow from requiring those binaries on `PATH` while we work on the complete integration.

You can experiment with the new command:

```bash
greentic-operator dev embedded --project-root /projects/ai/greentic-ng/greentic-dev
```

It starts the gateway/egress/subscriptions services (plus optional NATS) and blocks until you stop it with `Ctrl+C`.
The command honors `greentic.yaml` in the target project root and uses the same detection logic as `dev up`. Supply
`--no-nats` if you already have a NATS instance running.

`greentic-operator dev up` now runs the same embedded stack by default (including the gateway/egress/subscriptions
 trio), so it keeps running until you press `Ctrl+C` and cleans up NATS and event components before exiting.
