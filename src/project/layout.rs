use std::path::Path;

use super::{ensure_dir, write_if_missing};

const DEFAULT_GMAP: &str = "_ = forbidden\n";
const GREENTIC_YAML: &str = "\
# greentic operator project
# Optional binary overrides and dev-mode wiring
dev:
  mode: auto
  root: null
  profile: debug
  target_dir: null
  repo_map: {}
binaries: {}
";

pub fn ensure_layout(root: &Path) -> anyhow::Result<()> {
    ensure_dir(&root.join("providers"))?;
    ensure_dir(&root.join("providers").join("messaging"))?;
    ensure_dir(&root.join("packs"))?;
    ensure_dir(&root.join("tenants"))?;
    ensure_dir(&root.join("tenants").join("default"))?;
    ensure_dir(&root.join("tenants").join("default").join("teams"))?;
    ensure_dir(&root.join("state").join("resolved"))?;
    ensure_dir(&root.join("state").join("gtbind"))?;
    ensure_dir(&root.join("state").join("pids"))?;
    ensure_dir(&root.join("state").join("logs"))?;
    ensure_dir(&root.join("state").join("runs"))?;
    ensure_dir(&root.join("state").join("doctor"))?;

    write_if_missing(&root.join("greentic.yaml"), GREENTIC_YAML)?;
    write_if_missing(
        &root.join("tenants").join("default").join("tenant.gmap"),
        DEFAULT_GMAP,
    )?;

    Ok(())
}
