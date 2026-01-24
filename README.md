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
