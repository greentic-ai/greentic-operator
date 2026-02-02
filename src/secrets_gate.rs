use std::{
    collections::{BTreeMap, HashSet},
    env,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result as AnyhowResult, anyhow};
use async_trait::async_trait;
use greentic_secrets_lib::env::EnvSecretsManager;
use greentic_secrets_lib::{Result as SecretResult, SecretError, SecretsManager};
use serde::Deserialize;
use serde_cbor::value::Value as CborValue;
use tokio::runtime::Builder;
use tracing::info;
use zip::{ZipArchive, result::ZipError};

use crate::operator_log;
use crate::secret_name;
use crate::secrets_client::SecretsClient;
use crate::secrets_manager;

type CborMap = BTreeMap<CborValue, CborValue>;

pub type DynSecretsManager = Arc<dyn SecretsManager>;

struct LoggingSecretsManager {
    inner: DynSecretsManager,
    pack_id: Option<String>,
    dev_store_path_display: String,
    using_env_fallback: bool,
}

impl LoggingSecretsManager {
    fn new(
        inner: DynSecretsManager,
        pack_id: Option<String>,
        dev_store_path: Option<&Path>,
        using_env_fallback: bool,
    ) -> Self {
        let dev_store_path_display = dev_store_path
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<default>".to_string());
        Self {
            inner,
            pack_id,
            dev_store_path_display,
            using_env_fallback,
        }
    }

    fn alias_for(&self, uri: &str) -> Option<String> {
        let pack_id = self.pack_id.as_deref()?;
        rewrite_provider_segment(uri, pack_id)
    }
}

#[async_trait]
impl SecretsManager for LoggingSecretsManager {
    async fn read(&self, path: &str) -> SecretResult<Vec<u8>> {
        operator_log::info(
            module_path!(),
            format!(
                "WASM secrets read requested uri={path}; backend dev_store_path={} using_env_fallback={}",
                self.dev_store_path_display, self.using_env_fallback,
            ),
        );
        match self.inner.read(path).await {
            Ok(value) => Ok(value),
            Err(SecretError::NotFound(_)) => {
                if let Some(alias) = self.alias_for(path) {
                    operator_log::info(
                        module_path!(),
                        format!("WASM secrets alias attempt uri={alias}"),
                    );
                    return self.inner.read(&alias).await;
                }
                Err(SecretError::NotFound(path.to_string()))
            }
            Err(err) => Err(err),
        }
    }

    async fn write(&self, path: &str, value: &[u8]) -> SecretResult<()> {
        self.inner.write(path, value).await
    }

    async fn delete(&self, path: &str) -> SecretResult<()> {
        self.inner.delete(path).await
    }
}
const ENV_ALLOW_ENV_SECRETS: &str = "GREENTIC_ALLOW_ENV_SECRETS";

#[derive(Clone)]
pub struct SecretsManagerHandle {
    manager: DynSecretsManager,
    pub selection: secrets_manager::SecretsManagerSelection,
    pub dev_store_path: Option<PathBuf>,
    pub canonical_team: String,
    pub using_env_fallback: bool,
}

impl SecretsManagerHandle {
    pub fn manager(&self) -> DynSecretsManager {
        self.manager.clone()
    }

    pub fn runtime_manager(&self, pack_id: Option<&str>) -> DynSecretsManager {
        Arc::new(LoggingSecretsManager::new(
            self.manager(),
            pack_id.map(|id| id.to_string()),
            self.dev_store_path.as_deref(),
            self.using_env_fallback,
        ))
    }
}

pub fn resolve_secrets_manager(
    bundle_root: &Path,
    tenant: &str,
    team: Option<&str>,
) -> AnyhowResult<SecretsManagerHandle> {
    let canonical_team = secrets_manager::canonical_team(team);
    let team_owned = canonical_team.into_owned();
    let selection = secrets_manager::select_secrets_manager(bundle_root, tenant, &team_owned)?;
    let allow_env = matches!(env::var(ENV_ALLOW_ENV_SECRETS).as_deref(), Ok("1"));
    let (manager, store_path, using_env_fallback) = match SecretsClient::open(bundle_root) {
        Ok(client) => {
            let path = client.store_path().map(|path| path.to_path_buf());
            (Arc::new(client) as DynSecretsManager, path, false)
        }
        Err(err) => {
            if allow_env {
                operator_log::warn(
                    module_path!(),
                    format!(
                        "dev secrets store unavailable; falling back to env secrets backend: {err}"
                    ),
                );
                (Arc::new(EnvSecretsManager) as DynSecretsManager, None, true)
            } else {
                return Err(err);
            }
        }
    };
    if let Some(pack_path) = &selection.pack_path {
        let dev_store_desc = store_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<default>".to_string());
        operator_log::info(
            module_path!(),
            format!(
                "secrets manager selected: {} (shim: using dev store {})",
                pack_path.display(),
                dev_store_desc
            ),
        );
    }
    Ok(SecretsManagerHandle {
        manager,
        selection,
        dev_store_path: store_path,
        canonical_team: team_owned,
        using_env_fallback,
    })
}

/// Build the canonical secrets URI for the provided identity.
pub fn canonical_secret_uri(
    env: &str,
    tenant: &str,
    team: Option<&str>,
    provider: &str,
    key: &str,
) -> String {
    let team_segment = secrets_manager::canonical_team(team);
    let provider_segment = canonical_namespace(provider).unwrap_or_else(|| "messaging".to_string());
    let normalized_key = secret_name::canonical_secret_name(key);
    format!(
        "secrets://{}/{}/{}/{}/{}",
        env, tenant, team_segment, provider_segment, normalized_key
    )
}

fn secret_uri_candidates(
    env: &str,
    tenant: &str,
    canonical_team: &str,
    key: &str,
    pack_id: &str,
) -> Vec<String> {
    let normalized_key = secret_name::canonical_secret_name(key);
    let prefix = format!("secrets://{}/{}/{}/", env, tenant, canonical_team);
    vec![format!("{prefix}{pack_id}/{normalized_key}")]
}

fn display_secret_candidates(
    env: &str,
    tenant: &str,
    canonical_team: &str,
    key: &str,
    pack_id: &str,
) -> Vec<String> {
    let normalized_key = secret_name::canonical_secret_name(key);
    let prefix = format!("secrets://{}/{}/{}/", env, tenant, canonical_team);
    vec![format!("{prefix}{pack_id}/{normalized_key}")]
}

fn rewrite_provider_segment(uri: &str, pack_id: &str) -> Option<String> {
    let trimmed = uri.strip_prefix("secrets://")?;
    let mut segments = trimmed.splitn(5, '/').collect::<Vec<_>>();
    if segments.len() < 5 {
        return None;
    }
    if segments[3] == pack_id {
        return None;
    }
    segments[3] = pack_id;
    Some(format!("secrets://{}", segments.join("/")))
}

fn canonical_namespace(value: &str) -> Option<String> {
    let normalized = secret_name::canonical_secret_key_path(value);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Check that the required secrets for the provider exist.
#[allow(clippy::too_many_arguments)]
pub fn check_provider_secrets(
    manager: &DynSecretsManager,
    env: &str,
    tenant: &str,
    team: Option<&str>,
    pack_path: &Path,
    pack_id: &str,
    provider_id: &str,
    _provider_type: Option<&str>,
    store_path: Option<&Path>,
    using_env_fallback: bool,
) -> anyhow::Result<Option<Vec<String>>> {
    let keys = load_secret_keys_from_pack(pack_path)?;
    if keys.is_empty() {
        return Ok(None);
    }

    let canonical_team = secrets_manager::canonical_team(team);
    let canonical_team_owned = canonical_team.into_owned();
    let team_display = team.unwrap_or("default");
    let store_desc = store_path
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| {
            if using_env_fallback {
                "<env store>".to_string()
            } else {
                "<default dev store>".to_string()
            }
        });
    let store_path_display = store_path
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<none>".to_string());

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build secrets runtime")?;
    runtime.block_on(async {
        let mut missing = Vec::new();
        for key in keys {
            let normalized_key = secret_name::canonical_secret_name(&key);
            let candidates = secret_uri_candidates(
                env,
                tenant,
                &canonical_team_owned,
                &key,
                pack_id,
            );
            let display_candidates = display_secret_candidates(
                env,
                tenant,
                &canonical_team_owned,
                &key,
                pack_id,
            );
            if !display_candidates.is_empty() {
                let candidate_list = display_candidates
                    .iter()
                    .map(|uri| format!("  - {}", uri))
                    .collect::<Vec<_>>()
                    .join("\n");
                info!(
                    target: "secrets",
                    "checked secret URIs (store={} dev_store_path={}):\n{}",
                    store_desc,
                    store_path_display,
                    candidate_list
                );
            }
            let mut resolved = false;
            let mut candidate_missing = Vec::new();
            let mut matched_uri: Option<String> = None;
            for uri in &candidates {
                info!(
                    target: "secrets",
                    "secret lookup: uri={} secret_key={} dev_store_path={}",
                    uri,
                    normalized_key,
                    store_path_display
                );
                match manager.read(uri).await {
                    Ok(_) => {
                        resolved = true;
                        matched_uri = Some(uri.clone());
                        break;
                    }
                    Err(SecretError::NotFound(_)) => {
                        candidate_missing.push(uri.clone());
                    }
                    Err(err) => {
                        candidate_missing.push(uri.clone());
                        operator_log::warn(
                            module_path!(),
                            format!("secret lookup failed for {uri}: {err}"),
                        );
                    }
                }
            }
            let matched_display = matched_uri
                .as_deref()
                .map(|uri| uri.to_string())
                .unwrap_or_else(|| "<none>".to_string());
            operator_log::debug(
                module_path!(),
                format!(
                    "secrets: resolved {key}; store={} env={} tenant={} team={} canonical_team={} provider={} tried_keys={:?} matched_key={matched_display}",
                    store_desc,
                    env,
                    tenant,
                    team_display,
                    canonical_team_owned,
                    provider_id,
                    candidates
                ),
            );
            if !resolved {
                let display_set: HashSet<_> =
                    display_candidates.iter().collect();
                missing.extend(
                    candidate_missing
                        .into_iter()
                        .filter(|uri| display_set.contains(uri)),
                );
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
                if let Some(key_value) = map_get(entry_map, "key")
                    && let Some(key) =
                        resolve_string_symbol(Some(key_value), symbols, "secret_requirements")?
                {
                    keys.push(key.to_lowercase());
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
    if let Some(stripped) = key.strip_suffix('s')
        && let Some(CborValue::Array(values)) = map_get(symbols, stripped)
    {
        return Some(values);
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
    use greentic_secrets_lib::core::seed::{ApplyOptions, DevStore, apply_seed};
    use greentic_secrets_lib::{SecretFormat, SeedDoc, SeedEntry, SeedValue};
    use std::collections::HashMap;
    use std::env;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use tokio::runtime::Runtime;

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
        let uri = canonical_secret_uri("demo", "acme", None, "messaging", "FOO");
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
            "messaging-telegram",
            Some("messaging.telegram.bot"),
            None,
            false,
        )?;
        assert_eq!(
            result,
            Some(vec![
                "secrets://demo/tenant/_/messaging-telegram/telegram_bot_token".to_string()
            ])
        );
        Ok(())
    }

    #[test]
    fn provider_secrets_pass_when_supplied() -> anyhow::Result<()> {
        let mut values = HashMap::new();
        values.insert(
            "secrets://demo/tenant/_/messaging-telegram/telegram_bot_token".to_string(),
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
            "messaging-telegram",
            Some("messaging.telegram.bot"),
            None,
            false,
        )?;
        assert!(result.is_none());
        Ok(())
    }

    #[test]
    fn reads_provider_namespace_secret() -> anyhow::Result<()> {
        let dir = tempdir().unwrap();
        let store_path = dir.path().join("secrets.env");
        let store = DevStore::with_path(store_path.clone())?;
        let seed = SeedDoc {
            entries: vec![SeedEntry {
                uri: "secrets://demo/3point/_/messaging-telegram/telegram_bot_token".to_string(),
                format: SecretFormat::Text,
                value: SeedValue::Text {
                    text: "token".to_string(),
                },
                description: None,
            }],
        };
        let runtime = Runtime::new()?;
        let report =
            runtime.block_on(async { apply_seed(&store, &seed, ApplyOptions::default()).await });
        assert_eq!(report.ok, 1);
        unsafe {
            env::set_var("GREENTIC_DEV_SECRETS_PATH", store_path.clone());
        }
        let handle = resolve_secrets_manager(dir.path(), "3point", Some("default"))?;
        unsafe {
            env::remove_var("GREENTIC_DEV_SECRETS_PATH");
        }
        let missing = check_provider_secrets(
            &handle.manager(),
            "demo",
            "3point",
            Some("default"),
            &telegram_pack_path(),
            "messaging-telegram",
            "messaging-telegram",
            Some("messaging.telegram.bot"),
            handle.dev_store_path.as_deref(),
            handle.using_env_fallback,
        )?;
        assert!(missing.is_none());
        Ok(())
    }

    #[test]
    fn resolves_dev_store_secret_with_canonical_team() -> anyhow::Result<()> {
        let dir = tempdir().unwrap();
        let store_path = dir.path().join("secrets.env");
        let store = DevStore::with_path(store_path.clone())?;
        let seed = SeedDoc {
            entries: vec![SeedEntry {
                uri: "secrets://demo/3point/_/messaging-telegram/telegram_bot_token".to_string(),
                format: SecretFormat::Text,
                value: SeedValue::Text {
                    text: "XYZ".to_string(),
                },
                description: None,
            }],
        };
        let runtime = Runtime::new()?;
        let report =
            runtime.block_on(async { apply_seed(&store, &seed, ApplyOptions::default()).await });
        assert_eq!(report.ok, 1);
        unsafe {
            env::set_var("GREENTIC_DEV_SECRETS_PATH", store_path);
        }
        let handle = resolve_secrets_manager(dir.path(), "3point", Some("default"))?;
        unsafe {
            env::remove_var("GREENTIC_DEV_SECRETS_PATH");
        }
        let missing = check_provider_secrets(
            &handle.manager(),
            "demo",
            "3point",
            Some("default"),
            &telegram_pack_path(),
            "messaging-telegram",
            "messaging-telegram",
            Some("messaging.telegram.bot"),
            handle.dev_store_path.as_deref(),
            handle.using_env_fallback,
        )?;
        assert!(missing.is_none());
        Ok(())
    }

    #[test]
    fn secrets_handle_reads_dev_store_secret() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let store_path = dir.path().join("secrets.env");
        let store = DevStore::with_path(store_path.clone())?;
        let seed = SeedDoc {
            entries: vec![SeedEntry {
                uri: "secrets://demo/3point/_/messaging-telegram/telegram_bot_token".to_string(),
                format: SecretFormat::Text,
                value: SeedValue::Text {
                    text: "token".to_string(),
                },
                description: None,
            }],
        };
        let runtime = Runtime::new()?;
        let report =
            runtime.block_on(async { apply_seed(&store, &seed, ApplyOptions::default()).await });
        assert_eq!(report.ok, 1);
        unsafe {
            env::set_var("GREENTIC_DEV_SECRETS_PATH", store_path.clone());
        }
        let handle = resolve_secrets_manager(dir.path(), "demo", Some("default"))?;
        unsafe {
            env::remove_var("GREENTIC_DEV_SECRETS_PATH");
        }
        let value = runtime.block_on(async {
            handle
                .manager()
                .read("secrets://demo/3point/_/messaging-telegram/telegram_bot_token")
                .await
        })?;
        assert_eq!(value, b"token".to_vec());
        assert_eq!(handle.dev_store_path.as_deref(), Some(store_path.as_path()));
        Ok(())
    }
}
