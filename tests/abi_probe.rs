use std::path::PathBuf;

use anyhow::Result;
use greentic_runner_desktop::{RunOptions, TenantContext, run_pack_with_options};
use serde_json::json;

#[test]
fn runner_desktop_abi_probe() -> Result<()> {
    let pack_path = match std::env::var("GREENTIC_OPERATOR_ABI_TEST_PACK") {
        Ok(value) => PathBuf::from(value),
        Err(_) => {
            eprintln!("Skipping ABI probe; set GREENTIC_OPERATOR_ABI_TEST_PACK to a .gtpack path.");
            return Ok(());
        }
    };

    if !pack_path.exists() {
        return Err(anyhow::anyhow!(
            "ABI probe pack not found: {}",
            pack_path.display()
        ));
    }

    let input = json!({
        "tenant": "probe",
        "team": "default",
        "public_base_url": "https://example.com",
        "config": {
            "public_base_url": "https://example.com"
        }
    });

    let opts = RunOptions {
        entry_flow: Some("setup_default".to_string()),
        input,
        ctx: TenantContext {
            tenant_id: Some("probe".to_string()),
            team_id: Some("default".to_string()),
            user_id: Some("operator".to_string()),
            session_id: None,
        },
        dist_offline: true,
        ..RunOptions::default()
    };

    match run_pack_with_options(&pack_path, opts) {
        Ok(_) => Ok(()),
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("no exported instance named greentic:component/node@0.4.0") {
                return Err(anyhow::anyhow!("ABI mismatch detected: {msg}"));
            }
            Err(anyhow::anyhow!("{msg}"))
        }
    }
}
