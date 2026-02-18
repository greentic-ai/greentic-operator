use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use greentic_operator::dev_store_path;
use greentic_operator::operator_log::{self, Level};
use greentic_operator::secret_name;
use greentic_operator::secrets_gate;
use greentic_operator::secrets_manager;
use greentic_secrets_lib::{
    ApplyOptions, DevStore, SecretFormat, SeedDoc, SeedEntry, SeedValue, apply_seed,
};
use rand::RngExt;
use serde_cbor::to_vec;
use serde_json::json;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::tempdir;
use tokio::runtime::Runtime;
use zip::ZipWriter;
use zip::write::FileOptions;

#[test]
fn demo_send_redacts_runtime_secret_values() -> Result<()> {
    let bundle_root = tempdir()?;
    let log_dir = bundle_root.path().join("logs");
    fs::create_dir_all(&log_dir)?;
    operator_log::init(log_dir.clone(), Level::Debug)?;

    let pack_path = create_secret_pack(
        bundle_root.path(),
        "messaging-demo",
        &["telegram_bot_token"],
    )?;
    let secret_value = generate_test_secret();
    let pack_id = "messaging-demo";
    let secret_key = "telegram_bot_token";
    let uri = secret_pack_uri("demo", "demo-tenant", Some("default"), pack_id, secret_key);

    let store_path = dev_store_path::ensure_path(bundle_root.path())?;
    let store = DevStore::with_path(&store_path)?;
    let seed = SeedDoc {
        entries: vec![SeedEntry {
            uri,
            format: SecretFormat::Text,
            value: SeedValue::Text {
                text: secret_value.clone(),
            },
            description: None,
        }],
    };
    let runtime = Runtime::new()?;
    let report =
        runtime.block_on(async { apply_seed(&store, &seed, ApplyOptions::default()).await });
    assert_eq!(report.ok, 1);

    let secrets_handle =
        secrets_gate::resolve_secrets_manager(bundle_root.path(), "demo-tenant", Some("default"))?;
    let missing = secrets_gate::check_provider_secrets(
        &secrets_handle.manager(),
        "demo",
        "demo-tenant",
        Some("default"),
        &pack_path,
        "messaging-demo",
        Some("messaging.demo.bot"),
        secrets_handle.dev_store_path.as_deref(),
        secrets_handle.using_env_fallback,
    )?;
    assert!(missing.is_none());

    let provider_request = json!({
        "tenant": "demo-tenant",
        "team": "default",
        "env": "demo",
        "provider": "messaging-demo",
        "payload": { "text": "hello demo" },
    });
    assert_eq!(provider_request["tenant"], "demo-tenant");
    assert_eq!(provider_request["payload"]["text"], "hello demo");

    let contents = fs::read_to_string(log_dir.join("operator.log"))?;
    assert!(
        !contents.contains(&secret_value),
        "operator log leaked the secret value"
    );
    Ok(())
}

fn generate_test_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let encoded = URL_SAFE_NO_PAD.encode(bytes);
    format!("TEST_OPAQUE_{encoded}")
}

fn create_secret_pack(bundle_root: &Path, pack_id: &str, secret_keys: &[&str]) -> Result<PathBuf> {
    let pack_dir = bundle_root.join("providers").join("messaging");
    fs::create_dir_all(&pack_dir)?;
    let pack_path = pack_dir.join(format!("{pack_id}.gtpack"));
    let file = File::create(&pack_path)?;
    let mut zip = ZipWriter::new(file);
    let options: FileOptions<'_, ()> = FileOptions::default();
    zip.start_file("manifest.cbor", options)?;
    let manifest = json!({
        "meta": {
            "pack_id": pack_id,
            "entry_flows": ["setup_default"],
        }
    });
    zip.write_all(&to_vec(&manifest)?)?;
    if !secret_keys.is_empty() {
        zip.start_file("secret_requirements.json", options)?;
        let requirements: Vec<_> = secret_keys
            .iter()
            .map(|key| json!({ "key": key, "required": true }))
            .collect();
        zip.write_all(serde_json::to_string(&requirements)?.as_bytes())?;
    }
    zip.finish()?;
    Ok(pack_path)
}

fn secret_pack_uri(
    env: &str,
    tenant: &str,
    team: Option<&str>,
    pack_id: &str,
    key: &str,
) -> String {
    let canonical_team = secrets_manager::canonical_team(team);
    let normalized_key = secret_name::canonical_secret_name(key);
    format!(
        "secrets://{}/{}/{}/{}/{}",
        env, tenant, canonical_team, pack_id, normalized_key
    )
}
