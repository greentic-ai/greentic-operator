OP-EVENTS-PR-01 — Provider/domain auto-discovery + state persistence
Goal

Make greentic-operator automatically detect enabled domains based on provider packs present:

enable messaging if providers/messaging/*.gtpack exists

enable events if providers/events/*.gtpack exists

No --enable required. No “list providers” required.

Behavior

Default --project-dir .

Default providers_dir = <project-dir>/providers

Discovery returns:

domains_enabled = { messaging?, events? }

provider_packs = [{ domain, pack_path, provider_id }] (provider_id can be derived from filename if no manifest read)

Persist detection results:

state/runtime/<tenant>/detected_domains.json

state/runtime/<tenant>/detected_providers.json

What “validate if already implemented” means in this PR

Add tests that check current behavior:

If tests pass already → adjust code only to add persistence + docs

If tests fail → implement the missing discovery logic

Implementation details

Add discovery.rs module:

discover(project_dir) -> DiscoveryResult

stable ordering (sort by path)

Add DomainEnabledMode in config (optional in this PR):

auto|true|false (default auto)

Tests (mandatory)

Create temp dir with:

providers/messaging/a.gtpack

providers/events/b.gtpack

Assert discovery enables both domains and lists 2 packs.

Create temp dir with only events and assert only events enabled.

Acceptance criteria

✅ Running dev status shows detected domains/providers

✅ Discovery works without any flags

✅ Tests pass on Linux/macOS/Windows (path handling)