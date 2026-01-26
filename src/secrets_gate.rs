use std::{collections::BTreeMap, fs::File, io::Read, path::Path, sync::Arc};

use anyhow::{Context, anyhow};
use greentic_secrets_lib::env::EnvSecretsManager;
use greentic_secrets_lib::{SecretError, SecretsManager};
use serde::Deserialize;
use serde_cbor::value::Value as CborValue;
use tokio::runtime::Builder;
use zip::{ZipArchive, result::ZipError};

type CborMap = BTreeMap<CborValue, CborValue>;

pub type DynSecretsManager = Arc<dyn SecretsManager>;

/// Returns a basic secrets manager implementation suitable for the operator.
pub fn default_manager() -> DynSecretsManager {
    Arc::new(EnvSecretsManager)
}

/// Build the canonical secrets URI for the provided identity.
pub fn canonical_secret_uri(env: &str, tenant: &str, team: Option<&str>, key: &str) -> String {
    let team_segment = team.unwrap_or("_");
    let normalized_key = key.to_lowercase();
    format!(
        "secrets://{}/{}/{}/messaging/{}",
        env, tenant, team_segment, normalized_key
    )
}

/// Check that the required secrets for the provider exist.
pub fn check_provider_secrets(
    manager: &DynSecretsManager,
    env: &str,
    tenant: &str,
    team: Option<&str>,
    pack_path: &Path,
    _provider_id: &str,
) -> anyhow::Result<Option<Vec<String>>> {
    let keys = load_secret_keys_from_pack(pack_path)?;
    if keys.is_empty() {
        return Ok(None);
    }

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build secrets runtime")?;
    runtime.block_on(async {
        let mut missing = Vec::new();
        for key in keys {
            let uri = canonical_secret_uri(env, tenant, team, &key);
            match manager.read(&uri).await {
                Ok(_) => continue,
                Err(SecretError::NotFound(_)) => missing.push(uri),
                Err(err) => {
                    missing.push(uri.clone());
                    println!("[warn] secret lookup failed for {uri}: {err}");
                }
            }
        }
        if missing.is_empty() {
            Ok(None)
        } else {
            Ok(Some(missing))
        }
    })
}

fn load_secret_keys_from_pack(pack_path: &Path) -> anyhow::Result<Vec<String>> {
    let keys = load_keys_from_assets(pack_path)?;
    if !keys.is_empty() {
        return Ok(keys);
    }
    load_keys_from_manifest(pack_path)
}

fn load_keys_from_assets(pack_path: &Path) -> anyhow::Result<Vec<String>> {
    let file = File::open(pack_path)?;
    let mut archive = ZipArchive::new(file)?;
    const ASSET_PATHS: &[&str] = &[
        "assets/secret-requirements.json",
        "assets/secret_requirements.json",
        "secret-requirements.json",
        "secret_requirements.json",
    ];
    for asset in ASSET_PATHS {
        if let Ok(mut entry) = archive.by_name(asset) {
            let mut contents = String::new();
            entry.read_to_string(&mut contents)?;
            let requirements: Vec<AssetSecretRequirement> = serde_json::from_str(&contents)?;
            return Ok(requirements
                .into_iter()
                .filter(|req| req.required.unwrap_or(true))
                .filter_map(|req| req.key)
                .map(|key| key.to_lowercase())
                .collect());
        }
    }
    Ok(Vec::new())
}

fn load_keys_from_manifest(pack_path: &Path) -> anyhow::Result<Vec<String>> {
    let file = File::open(pack_path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut manifest = match archive.by_name("manifest.cbor") {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut bytes = Vec::new();
    manifest.read_to_end(&mut bytes)?;
    let value: CborValue = serde_cbor::from_slice(&bytes)?;
    if let CborValue::Map(map) = &value {
        return extract_keys_from_manifest_map(map);
    }
    Ok(Vec::new())
}

fn extract_keys_from_manifest_map(map: &CborMap) -> anyhow::Result<Vec<String>> {
    let symbols = symbols_map(map);
    let mut keys = Vec::new();
    if let Some(CborValue::Array(entries)) = map_get(map, "secret_requirements") {
        for entry in entries {
            if let CborValue::Map(entry_map) = entry {
                if !is_required(entry_map) {
                    continue;
                }
                if let Some(key_value) = map_get(entry_map, "key") {
                    if let Some(key) =
                        resolve_string_symbol(Some(key_value), symbols, "secret_requirements")?
                    {
                        keys.push(key.to_lowercase());
                    }
                }
            }
        }
    }
    Ok(keys)
}

fn is_required(entry: &CborMap) -> bool {
    match map_get(entry, "required") {
        Some(CborValue::Bool(value)) => *value,
        _ => true,
    }
}

fn map_get<'a>(map: &'a CborMap, key: &str) -> Option<&'a CborValue> {
    map.iter().find_map(|(k, v)| match k {
        CborValue::Text(text) if text == key => Some(v),
        _ => None,
    })
}

fn symbols_map(map: &CborMap) -> Option<&CborMap> {
    let symbols = map_get(map, "symbols")?;
    match symbols {
        CborValue::Map(map) => Some(map),
        _ => None,
    }
}

fn resolve_string_symbol(
    value: Option<&CborValue>,
    symbols: Option<&CborMap>,
    symbol_key: &str,
) -> anyhow::Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        CborValue::Text(text) => Ok(Some(text.clone())),
        CborValue::Integer(idx) => {
            let Some(symbols) = symbols else {
                return Ok(Some(idx.to_string()));
            };
            let Some(values) = symbol_array(symbols, symbol_key) else {
                return Ok(Some(idx.to_string()));
            };
            let idx = usize::try_from(*idx).unwrap_or(usize::MAX);
            match values.get(idx) {
                Some(CborValue::Text(text)) => Ok(Some(text.clone())),
                _ => Ok(Some(idx.to_string())),
            }
        }
        _ => Err(anyhow!("expected string or symbol index")),
    }
}

fn symbol_array<'a>(symbols: &'a CborMap, key: &'a str) -> Option<&'a Vec<CborValue>> {
    if let Some(CborValue::Array(values)) = map_get(symbols, key) {
        return Some(values);
    }
    if let Some(stripped) = key.strip_suffix('s') {
        if let Some(CborValue::Array(values)) = map_get(symbols, stripped) {
            return Some(values);
        }
    }
    None
}

#[derive(Deserialize)]
struct AssetSecretRequirement {
    key: Option<String>,
    #[serde(default)]
    required: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use greentic_secrets_lib::Result as SecretResult;
    use std::collections::HashMap;
    use std::path::PathBuf;

    struct FakeManager {
        values: HashMap<String, Vec<u8>>,
    }

    impl FakeManager {
        fn new(values: HashMap<String, Vec<u8>>) -> Self {
            Self { values }
        }
    }

    #[async_trait]
    impl SecretsManager for FakeManager {
        async fn read(&self, path: &str) -> SecretResult<Vec<u8>> {
            self.values
                .get(path)
                .cloned()
                .ok_or_else(|| SecretError::NotFound(path.to_string()))
        }

        async fn write(&self, _: &str, _: &[u8]) -> SecretResult<()> {
            Err(SecretError::Permission("read-only".into()))
        }

        async fn delete(&self, _: &str) -> SecretResult<()> {
            Err(SecretError::Permission("read-only".into()))
        }
    }

    fn telegram_pack_path() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("tests/demo-bundle/providers/messaging/messaging-telegram.gtpack");
        path.canonicalize().unwrap_or(path)
    }

    #[test]
    fn canonical_uri_uses_team_placeholder() {
        let uri = canonical_secret_uri("demo", "acme", None, "FOO");
        assert_eq!(uri, "secrets://demo/acme/_/messaging/foo");
    }

    #[test]
    fn provider_secrets_missing_when_unsupported() -> anyhow::Result<()> {
        let manager: DynSecretsManager = Arc::new(FakeManager::new(HashMap::new()));
        let result = check_provider_secrets(
            &manager,
            "demo",
            "tenant",
            Some("default"),
            &telegram_pack_path(),
            "messaging-telegram",
        )?;
        assert_eq!(
            result,
            Some(vec![
                "secrets://demo/tenant/default/messaging/telegram_bot_token".to_string()
            ])
        );
        Ok(())
    }

    #[test]
    fn provider_secrets_pass_when_supplied() -> anyhow::Result<()> {
        let mut values = HashMap::new();
        values.insert(
            "secrets://demo/tenant/_/messaging/telegram_bot_token".to_string(),
            b"token".to_vec(),
        );
        let manager: DynSecretsManager = Arc::new(FakeManager::new(values));
        let result = check_provider_secrets(
            &manager,
            "demo",
            "tenant",
            None,
            &telegram_pack_path(),
            "messaging-telegram",
        )?;
        assert!(result.is_none());
        Ok(())
    }
}
