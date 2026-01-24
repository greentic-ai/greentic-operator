# OP-PR-11B — Cloudflared quick tunnel + public URL discovery

## Goal
Add support for starting **cloudflared quick tunnel** during `demo up`, discovering the public URL, and persisting it for later steps (provider setup, webhook URLs, etc).

Builds on OP-PR-11A supervisor + runtime state.

## User stories
- As a developer, I can run `greentic-operator demo up` and it starts cloudflared and prints a usable public base URL.
- As a developer, I can re-run `demo up` and if cloudflared is already running, it reuses the existing URL unless `--restart cloudflared`.

## CLI additions
- `greentic-operator demo up ... [--cloudflared on|off] [--cloudflared-binary <path>] [--restart cloudflared]`

## Runtime artifacts
- `state/runtime/<tenant>.<team>/public_base_url.txt`
- `state/runtime/<tenant>.<team>/resolved/cloudflared.json`
- `state/logs/<tenant>.<team>/cloudflared.log`
- `state/pids/<tenant>.<team>/cloudflared.pid`

## Implementation plan
### 1) Add `cloudflared` module
- `CloudflaredSpec { binary: PathBuf, local_port: u16, extra_args: Vec<String> }`
- `start_quick_tunnel(paths, spec) -> Result<CloudflaredHandle>`
  - Spawn via supervisor with log capture
  - Read log file incrementally until URL discovered (or timeout)
  - URL regex: `https://[a-z0-9-]+\.trycloudflare\.com`
  - Persist the first match to `public_base_url.txt`
  - Return handle with URL

Avoid depending on exact log prefixes. Regex scan over appended log content.

### 2) Make URL discovery testable
Provide a fixture `fake_cloudflared` binary that:
- prints some lines
- prints a trycloudflare URL after a short delay
- sleeps
In tests, set `--cloudflared-binary` to point to this fixture.

### 3) Wire into `demo up` skeleton
If `demo up` doesn’t exist yet, implement a minimal `demo up` that starts only cloudflared and prints URL.

### 4) Restart / reuse logic
- If pidfile exists and process is running:
  - if `public_base_url.txt` exists -> reuse
  - else attempt discovery again from log file
- If `--restart cloudflared` -> stop + start

## Files to add/change
- `src/cloudflared.rs`
- `src/cli/demo_up.rs` (start cloudflared)
- `tests/cloudflared_url_discovery.rs`
- `tests/fixtures/fake_cloudflared/`

## Acceptance criteria
- ✅ `demo up` prints `Public URL: https://....trycloudflare.com`
- ✅ URL persisted to `public_base_url.txt`
- ✅ `demo status` shows cloudflared running
- ✅ Tests pass using fake_cloudflared
