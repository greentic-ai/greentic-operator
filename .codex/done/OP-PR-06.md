Implement OP-PR-06: add an ultra-optimised GitHub Actions workflow `.github/workflows/publish.yml`.

Requirements:
1) Tooling bootstrap in each job:
   - setup Rust stable with rustfmt + clippy components
   - install cargo-binstall (cache-friendly)
   - install required Greentic CLIs via cargo-binstall (at minimum greentic-pack; keep list explicit)
   - cache ~/.cargo/registry, ~/.cargo/git, and target/ keyed by OS + Cargo.lock hash
   - keep runs fast and deterministic

2) Parallel quality gates (separate jobs):
   - fmt: cargo fmt --check
   - clippy: cargo clippy -- -D warnings
   - test: cargo test
   These must run in parallel and block publishing.

3) After gates pass, start two things in parallel:
   A) Publish crates to crates.io using secret CARGO_REGISTRY_TOKEN.
      - Ensure idempotency (if already published, do not fail the whole workflow).
   B) Build & package binstall artifacts in a matrix for:
      - macOS 15 arm64 (macos-15)
      - macOS 15 x86_64 (prefer self-hosted intel if configured; otherwise cross-build x86_64-apple-darwin and document)
      - Windows x86_64 (windows-latest)
      - Linux x86_64 (ubuntu-latest)
      Upload artifacts and attach to a GitHub Release (or upload as workflow artifacts if release creation is not available).

4) Triggers:
   - on push to master
   - include workflow_dispatch

5) Concurrency protection:
   - concurrency group publish-${{ github.ref }}
   - do not cancel in progress

6) Keep changes CI-only. Add README notes in the repo if needed.

Do as much as possible without asking permission; only ask if a destructive change is required.

Update OP-PR-06 as follows:

1) Publishing must remove ALL path dependencies before pushing to crates.io.
   - Add `ci/prepare_publish_workspace.sh` that creates a temp publish workspace and rewrites all Cargo.toml files to remove any `path = ...` keys (keeping version = "0.4" etc).
   - After rewriting, verify no `path =` remains anywhere (fail fast).
   - Publish job must run from the publish workspace, not from the dev manifests.
   - Add a `--dry-run` mode that only prepares + validates without publishing.

2) Add `ci/local_checks.sh` that validates locally that GitHub flows will work before pushing:
   - Runs `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`
   - Runs `ci/prepare_publish_workspace.sh --dry-run`
   - In the prepared workspace, runs `cargo publish --dry-run` for the publishable crate(s)
   - Validates the binstall packaging script logic at least for the host platform (build release binary + package step)
   - Must exit non-zero on any failure

3) Update `.github/workflows/publish.yml`:
   - fmt/clippy/test remain parallel and are required gates.
   - publish-crates job uses `ci/prepare_publish_workspace.sh` before `cargo publish`.
   - Ensure `CARGO_REGISTRY_TOKEN` is only used in publish job.
   - Keep changes CI-only besides adding the ci scripts.

Do not introduce patch.crates-io. Keep version line at 0.4. Keep everything deterministic and fast.

In ci/prepare_publish_workspace.sh, prefer blanket removal of path = ... keys rather than trying to special-case only greentic crates. As long as version exists, it’s safe and future-proof.