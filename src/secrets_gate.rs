use async_trait::async_trait;
use std::{
    collections::{BTreeMap, HashSet},
    env,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result as AnyhowResult, anyhow};
use greentic_secrets_lib::env::EnvSecretsManager;
use greentic_secrets_lib::{
    Result as SecretResult, SecretError, SecretsManager, SecretsStore,
    core::{Error as CoreError, seed::DevStore},
};
use serde::Deserialize;
use serde_cbor::value::Value as CborValue;
use tokio::runtime::Builder;
use zip::{ZipArchive, result::ZipError};

use crate::operator_log;
use crate::secret_name;
use crate::secrets_manager;

type CborMap = BTreeMap<CborValue, CborValue>;

pub type DynSecretsManager = Arc<dyn SecretsManager>;

/// Returns a basic secrets manager implementation suitable for the operator.
pub fn default_manager() -> DynSecretsManager {
    Arc::new(EnvSecretsManager)
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
    let (manager, store_path, using_env_fallback) = match DevStoreSecretsManager::open() {
        Ok(store) => {
            let path = store.store_path();
            (Arc::new(store) as DynSecretsManager, path, false)
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

#[derive(Clone)]
struct DevStoreSecretsManager {
    store: Arc<DevStore>,
    store_path: Option<PathBuf>,
}

impl DevStoreSecretsManager {
    fn open() -> AnyhowResult<Self> {
        if let Some(path) = override_dev_store_path().or_else(find_default_dev_store_path)
            && path.exists()
        {
            return open_with_path(path);
        }
        let store = DevStore::open_default()
            .map_err(|err| anyhow!("failed to open dev secrets store: {err}"))?;
        Ok(Self {
            store: Arc::new(store),
            store_path: None,
        })
    }

    fn store_path(&self) -> Option<PathBuf> {
        self.store_path.clone()
    }
}

fn override_dev_store_path() -> Option<PathBuf> {
    env::var("GREENTIC_DEV_SECRETS_PATH")
        .ok()
        .map(PathBuf::from)
}

fn find_default_dev_store_path() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    let candidates = [
        cwd.join(".greentic/dev/.dev.secrets.env"),
        cwd.join(".greentic/state/dev/.dev.secrets.env"),
    ];
    candidates.into_iter().find(|path| path.exists())
}

fn open_with_path(path: PathBuf) -> AnyhowResult<DevStoreSecretsManager> {
    let store = DevStore::with_path(path.clone())
        .map_err(|err| anyhow!("failed to open dev secrets store: {err}"))?;
    Ok(DevStoreSecretsManager {
        store: Arc::new(store),
        store_path: Some(path),
    })
}

#[async_trait]
impl SecretsManager for DevStoreSecretsManager {
    async fn read(&self, path: &str) -> SecretResult<Vec<u8>> {
        let context = SecretResolutionContext::from_path(path);
        let store_desc = self
            .store_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<default>".to_string());
        let mut matched_candidate: Option<String> = None;
        eprintln!(
            "secrets_gate::DevStore read env={:?} tenant={:?} team={:?} canonical_team={:?} candidates={:?} store={} path={}",
            context.env,
            context.tenant,
            context.team,
            context.canonical_team,
            context.candidates,
            store_desc,
            path
        );
        for candidate in &context.candidates {
            match self.store.get(candidate).await {
                Ok(value) => {
                    matched_candidate = Some(candidate.clone());
                    log_secret_resolution(&context, &store_desc, matched_candidate.as_deref());
                    eprintln!(
                        "secrets_gate::DevStore matched candidate={matched_candidate:?} path={path}"
                    );
                    return Ok(value);
                }
                Err(CoreError::NotFound { .. }) => continue,
                Err(err) => {
                    log_secret_resolution(&context, &store_desc, matched_candidate.as_deref());
                    return Err(map_dev_error(err));
                }
            }
        }
        log_secret_resolution(&context, &store_desc, matched_candidate.as_deref());
        Err(SecretError::NotFound(
            context
                .candidates
                .first()
                .cloned()
                .unwrap_or_else(|| path.to_string()),
        ))
    }

    async fn write(&self, _: &str, _: &[u8]) -> SecretResult<()> {
        Err(SecretError::Permission(
            "dev secrets store is read-only".into(),
        ))
    }

    async fn delete(&self, _: &str) -> SecretResult<()> {
        Err(SecretError::Permission(
            "dev secrets store is read-only".into(),
        ))
    }
}

fn map_dev_error(err: CoreError) -> SecretError {
    match err {
        CoreError::NotFound { entity } => SecretError::NotFound(entity),
        other => SecretError::Backend(other.to_string().into()),
    }
}

struct SecretResolutionContext {
    env: Option<String>,
    tenant: Option<String>,
    team: Option<String>,
    canonical_team: Option<String>,
    secret_name: String,
    candidates: Vec<String>,
}

impl SecretResolutionContext {
    fn from_path(path: &str) -> Self {
        let mut context = Self {
            env: None,
            tenant: None,
            team: None,
            canonical_team: None,
            secret_name: path.to_string(),
            candidates: vec![path.to_string()],
        };
        if let Some(parsed) = parse_runtime_secret_path(path) {
            let canonical_team =
                secrets_manager::canonical_team(Some(parsed.team.as_str())).into_owned();
            let normalized_key = secret_name::canonical_secret_name(&parsed.secret);
            let prefix = format!(
                "secrets://{}/{}/{}/",
                parsed.env, parsed.tenant, canonical_team
            );
            context.env = Some(parsed.env.clone());
            context.tenant = Some(parsed.tenant.clone());
            context.team = Some(parsed.team.clone());
            context.canonical_team = Some(canonical_team.clone());
            context.secret_name = normalized_key.clone();
            const CANDIDATE_SUFFIXES: &[&str] = &[
                "messaging",
                "messaging-telegram",
                "messaging.telegram.bot",
                "messaging/telegram/bot",
                "configs",
            ];
            let mut candidates = Vec::with_capacity(1 + CANDIDATE_SUFFIXES.len());
            candidates.push(format!("{prefix}kv/{normalized_key}"));
            for suffix in CANDIDATE_SUFFIXES {
                candidates.push(format!("{prefix}{suffix}/{normalized_key}"));
            }
            context.candidates = candidates;
        }
        context
    }
}

fn log_secret_resolution(ctx: &SecretResolutionContext, store_desc: &str, matched: Option<&str>) {
    let env = ctx.env.as_deref().unwrap_or("<unknown>");
    let tenant = ctx.tenant.as_deref().unwrap_or("<unknown>");
    let team = ctx.team.as_deref().unwrap_or("<unknown>");
    let canonical_team = ctx.canonical_team.as_deref().unwrap_or("<unknown>");
    let matched_display = matched.unwrap_or("<none>");
    operator_log::debug(
        module_path!(),
        format!(
            "secrets: resolving {} store={} env={} tenant={} team={} canonical_team={} tried_keys={:?} matched_key={matched_display}",
            ctx.secret_name, store_desc, env, tenant, team, canonical_team, ctx.candidates
        ),
    );
}

struct ParsedRuntimeSecretPath {
    env: String,
    tenant: String,
    team: String,
    secret: String,
}

fn parse_runtime_secret_path(path: &str) -> Option<ParsedRuntimeSecretPath> {
    let trimmed = path.strip_prefix("secrets://")?;
    let mut segments = trimmed.split('/');
    let env = segments.next()?.to_string();
    let tenant = segments.next()?.to_string();
    let team = segments.next()?.to_string();
    let category = segments.next()?;
    if category != "kv" {
        return None;
    }
    let remaining: Vec<&str> = segments.collect();
    if remaining.is_empty() {
        return None;
    }
    let secret = remaining.join("/");
    Some(ParsedRuntimeSecretPath {
        env,
        tenant,
        team,
        secret,
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
    provider_id: &str,
    provider_type: Option<&str>,
) -> Vec<String> {
    let normalized_key = secret_name::canonical_secret_name(key);
    let prefix = format!("secrets://{}/{}/{}/", env, tenant, canonical_team);
    let mut candidates = Vec::new();
    if let Some(namespace) = canonical_namespace(provider_id) {
        candidates.push(format!("{prefix}{namespace}/{normalized_key}"));
    }
    if let Some(provider_type) = provider_type {
        if let Some(namespace) = canonical_namespace(provider_type) {
            if !namespace.is_empty() {
                candidates.push(format!("{prefix}{namespace}/{normalized_key}"));
            }
        } else if !provider_type.trim().is_empty() {
            candidates.push(format!("{prefix}{provider_type}/{normalized_key}"));
        }
    }
    candidates.push(format!("{prefix}messaging/{normalized_key}"));
    const CANDIDATE_SUFFIXES: &[&str] = &[
        "messaging-telegram",
        "messaging.telegram.bot",
        "messaging/telegram/bot",
        "configs",
        "kv",
    ];
    for suffix in CANDIDATE_SUFFIXES {
        candidates.push(format!("{prefix}{suffix}/{normalized_key}"));
    }
    candidates
}

fn display_secret_candidates(
    env: &str,
    tenant: &str,
    canonical_team: &str,
    key: &str,
    provider_id: &str,
    provider_type: Option<&str>,
) -> Vec<String> {
    let normalized_key = secret_name::canonical_secret_name(key);
    let prefix = format!("secrets://{}/{}/{}/", env, tenant, canonical_team);
    let mut candidates = Vec::new();
    let mut push_namespace = |namespace: &str| {
        let trimmed = namespace.trim();
        if trimmed.is_empty() {
            return;
        }
        let candidate = format!("{prefix}{trimmed}/{normalized_key}");
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    };
    push_namespace("messaging");
    push_namespace(provider_id);
    if let Some(provider_type) = provider_type {
        push_namespace(provider_type);
    }
    const DISPLAY_SUFFIXES: &[&str] = &[
        "messaging-telegram",
        "messaging.telegram.bot",
        "messaging/telegram/bot",
    ];
    for suffix in DISPLAY_SUFFIXES {
        push_namespace(suffix);
    }
    candidates
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
    provider_id: &str,
    provider_type: Option<&str>,
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

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build secrets runtime")?;
    runtime.block_on(async {
        let mut missing = Vec::new();
        for key in keys {
            let candidates = secret_uri_candidates(
                env,
                tenant,
                &canonical_team_owned,
                &key,
                provider_id,
                provider_type,
            );
            let display_candidates = display_secret_candidates(
                env,
                tenant,
                &canonical_team_owned,
                &key,
                provider_id,
                provider_type,
            );
            let mut resolved = false;
            let mut candidate_missing = Vec::new();
            let mut matched_uri: Option<String> = None;
            for uri in &candidates {
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
    use greentic_secrets_lib::core::seed::{ApplyOptions, apply_seed};
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
            Some("messaging.telegram.bot"),
            None,
            false,
        )?;
        assert_eq!(
            result,
            Some(vec![
                "secrets://demo/tenant/_/messaging/telegram_bot_token".to_string(),
                "secrets://demo/tenant/_/messaging-telegram/telegram_bot_token".to_string(),
                "secrets://demo/tenant/_/messaging.telegram.bot/telegram_bot_token".to_string(),
                "secrets://demo/tenant/_/messaging/telegram/bot/telegram_bot_token".to_string(),
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
                uri: "secrets://demo/3point/_/messaging/telegram_bot_token".to_string(),
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
            Some("messaging.telegram.bot"),
            handle.dev_store_path.as_deref(),
            handle.using_env_fallback,
        )?;
        assert!(missing.is_none());
        Ok(())
    }
}
