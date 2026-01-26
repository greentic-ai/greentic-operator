use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_cbor::Value as CborValue;
use zip::result::ZipError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Domain {
    Messaging,
    Events,
    Secrets,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainAction {
    Setup,
    Diagnostics,
    Verify,
}

#[derive(Clone, Debug)]
pub struct DomainConfig {
    pub providers_dir: &'static str,
    pub setup_flow: &'static str,
    pub diagnostics_flow: &'static str,
    pub verify_flows: &'static [&'static str],
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderPack {
    pub pack_id: String,
    pub file_name: String,
    pub path: PathBuf,
    pub entry_flows: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlannedRun {
    pub pack: ProviderPack,
    pub flow_id: String,
}

pub fn config(domain: Domain) -> DomainConfig {
    match domain {
        Domain::Messaging => DomainConfig {
            providers_dir: "providers/messaging",
            setup_flow: "setup_default",
            diagnostics_flow: "diagnostics",
            verify_flows: &["verify_webhooks"],
        },
        Domain::Events => DomainConfig {
            providers_dir: "providers/events",
            setup_flow: "setup_default",
            diagnostics_flow: "diagnostics",
            verify_flows: &["verify_subscriptions"],
        },
        Domain::Secrets => DomainConfig {
            providers_dir: "providers/secrets",
            setup_flow: "setup_default",
            diagnostics_flow: "diagnostics",
            verify_flows: &[],
        },
    }
}

pub fn validator_pack_path(root: &Path, domain: Domain) -> Option<PathBuf> {
    let name = match domain {
        Domain::Messaging => "validators-messaging.gtpack",
        Domain::Events => "validators-events.gtpack",
        Domain::Secrets => "validators-secrets.gtpack",
    };
    let path = root.join("validators").join(domain_name(domain)).join(name);
    if path.exists() { Some(path) } else { None }
}

pub fn ensure_cbor_packs(root: &Path) -> anyhow::Result<()> {
    let mut roots = Vec::new();
    let providers = root.join("providers");
    if providers.exists() {
        roots.push(providers);
    }
    let packs = root.join("packs");
    if packs.exists() {
        roots.push(packs);
    }
    for root in roots {
        for pack in collect_gtpacks(&root)? {
            let file = std::fs::File::open(&pack)?;
            let mut archive = zip::ZipArchive::new(file)?;
            let manifest = read_manifest_cbor(&mut archive).map_err(|err| {
                anyhow::anyhow!(
                    "failed to decode manifest.cbor in {}: {err}",
                    pack.display()
                )
            })?;
            if manifest.is_none() {
                return Err(missing_cbor_error(&pack));
            }
        }
    }
    Ok(())
}

pub fn manifest_cbor_issue_detail(path: &Path) -> anyhow::Result<Option<String>> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut manifest = match archive.by_name("manifest.cbor") {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => {
            return Ok(Some("manifest.cbor missing from archive".to_string()));
        }
        Err(err) => return Err(err.into()),
    };
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut manifest, &mut bytes)?;
    let value = match serde_cbor::from_slice::<CborValue>(&bytes) {
        Ok(value) => value,
        Err(err) => return Ok(Some(err.to_string())),
    };
    if let Some(path) = find_manifest_string_type_mismatch(&value) {
        return Ok(Some(format!("invalid type at {path} (expected string)")));
    }
    Ok(None)
}

fn collect_gtpacks(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut packs = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) == Some("gtpack") {
                packs.push(path);
            }
        }
    }
    Ok(packs)
}

pub fn discover_provider_packs(root: &Path, domain: Domain) -> anyhow::Result<Vec<ProviderPack>> {
    let cfg = config(domain);
    let providers_dir = root.join(cfg.providers_dir);
    let mut packs = Vec::new();
    if !providers_dir.exists() {
        return Ok(packs);
    }
    for entry in std::fs::read_dir(&providers_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("gtpack") {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        let manifest = read_pack_manifest(&path)?;
        let meta = manifest
            .meta
            .ok_or_else(|| anyhow::anyhow!("pack manifest missing meta in {}", path.display()))?;
        packs.push(ProviderPack {
            pack_id: meta.pack_id,
            file_name,
            path,
            entry_flows: meta.entry_flows,
        });
    }
    packs.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(packs)
}

pub fn discover_provider_packs_cbor_only(
    root: &Path,
    domain: Domain,
) -> anyhow::Result<Vec<ProviderPack>> {
    let cfg = config(domain);
    let providers_dir = root.join(cfg.providers_dir);
    let mut packs = Vec::new();
    if !providers_dir.exists() {
        return Ok(packs);
    }
    for entry in std::fs::read_dir(&providers_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("gtpack") {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        let manifest = read_pack_manifest_cbor_only(&path)?;
        let meta = manifest
            .meta
            .ok_or_else(|| anyhow::anyhow!("pack manifest missing meta in {}", path.display()))?;
        packs.push(ProviderPack {
            pack_id: meta.pack_id,
            file_name,
            path,
            entry_flows: meta.entry_flows,
        });
    }
    packs.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(packs)
}

pub fn plan_runs(
    domain: Domain,
    action: DomainAction,
    packs: &[ProviderPack],
    provider_filter: Option<&str>,
    allow_missing_setup: bool,
) -> anyhow::Result<Vec<PlannedRun>> {
    let cfg = config(domain);
    let flows: Vec<&str> = match action {
        DomainAction::Setup => vec![cfg.setup_flow],
        DomainAction::Diagnostics => vec![cfg.diagnostics_flow],
        DomainAction::Verify => cfg.verify_flows.to_vec(),
    };

    let mut plan = Vec::new();
    for pack in packs {
        if let Some(filter) = provider_filter {
            let file_stem = pack
                .file_name
                .strip_suffix(".gtpack")
                .unwrap_or(&pack.file_name);
            let matches = pack.pack_id == filter
                || pack.file_name == filter
                || file_stem == filter
                || pack.pack_id.contains(filter)
                || pack.file_name.contains(filter)
                || file_stem.contains(filter);
            if !matches {
                continue;
            }
        }

        for flow in &flows {
            let has_flow = pack.entry_flows.iter().any(|entry| entry == flow);
            if !has_flow {
                if action == DomainAction::Setup && !allow_missing_setup {
                    return Err(anyhow::anyhow!(
                        "Missing required flow '{}' in provider pack {}",
                        flow,
                        pack.file_name
                    ));
                }
                eprintln!(
                    "Warning: provider pack {} missing flow {}; skipping.",
                    pack.file_name, flow
                );
                continue;
            }
            plan.push(PlannedRun {
                pack: pack.clone(),
                flow_id: (*flow).to_string(),
            });
        }
    }
    Ok(plan)
}

#[derive(Debug, Deserialize)]
pub(crate) struct PackManifestForDiscovery {
    #[serde(default)]
    pub meta: Option<PackMeta>,
    #[serde(default)]
    pub pack_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PackManifest {
    #[serde(default)]
    meta: Option<PackMeta>,
    #[serde(default)]
    pack_id: Option<String>,
    #[serde(default)]
    flows: Vec<PackFlow>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PackMeta {
    pub pack_id: String,
    #[serde(default)]
    entry_flows: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PackFlow {
    id: String,
    #[serde(default)]
    entrypoints: Vec<String>,
}

fn read_pack_manifest(path: &Path) -> anyhow::Result<PackManifest> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let manifest = read_pack_manifest_data(&mut archive, path)
        .with_context(|| format!("failed to read pack manifest from {}", path.display()))?;
    let pack_id = if let Some(meta) = manifest.meta.as_ref() {
        meta.pack_id.clone()
    } else if let Some(pack_id) = manifest.pack_id.as_ref() {
        pack_id.clone()
    } else {
        let fallback = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("pack")
            .to_string();
        eprintln!(
            "Warning: pack manifest missing pack id; using filename '{}' for {}",
            fallback,
            path.display()
        );
        fallback
    };
    let mut entry_flows = if let Some(meta) = manifest.meta.as_ref() {
        meta.entry_flows.clone()
    } else {
        Vec::new()
    };
    if entry_flows.is_empty() {
        for flow in &manifest.flows {
            entry_flows.push(flow.id.clone());
            for entry in &flow.entrypoints {
                entry_flows.push(entry.clone());
            }
        }
    }
    if entry_flows.is_empty() {
        entry_flows.push(pack_id.clone());
    }
    Ok(PackManifest {
        meta: Some(PackMeta {
            pack_id,
            entry_flows,
        }),
        pack_id: None,
        flows: Vec::new(),
    })
}

fn read_pack_manifest_cbor_only(path: &Path) -> anyhow::Result<PackManifest> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let manifest = match read_manifest_cbor(&mut archive).map_err(|err| {
        anyhow::anyhow!(
            "failed to decode manifest.cbor in {}: {err}",
            path.display()
        )
    })? {
        Some(manifest) => manifest,
        None => return Err(missing_cbor_error(path)),
    };
    let pack_id = if let Some(meta) = manifest.meta.as_ref() {
        meta.pack_id.clone()
    } else if let Some(pack_id) = manifest.pack_id.as_ref() {
        pack_id.clone()
    } else {
        let fallback = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("pack")
            .to_string();
        eprintln!(
            "Warning: pack manifest missing pack id; using filename '{}' for {}",
            fallback,
            path.display()
        );
        fallback
    };
    let mut entry_flows = if let Some(meta) = manifest.meta.as_ref() {
        meta.entry_flows.clone()
    } else {
        Vec::new()
    };
    if entry_flows.is_empty() {
        for flow in &manifest.flows {
            entry_flows.push(flow.id.clone());
            for entry in &flow.entrypoints {
                entry_flows.push(entry.clone());
            }
        }
    }
    if entry_flows.is_empty() {
        entry_flows.push(pack_id.clone());
    }
    Ok(PackManifest {
        meta: Some(PackMeta {
            pack_id,
            entry_flows,
        }),
        pack_id: None,
        flows: Vec::new(),
    })
}

fn read_pack_manifest_data(
    archive: &mut zip::ZipArchive<std::fs::File>,
    path: &Path,
) -> anyhow::Result<PackManifest> {
    match read_manifest_cbor(archive) {
        Ok(Some(manifest)) => return Ok(manifest),
        Ok(None) => {}
        Err(err) => {
            return Err(anyhow::anyhow!(
                "failed to decode manifest.cbor in {}: {err}",
                path.display()
            ));
        }
    }
    match read_manifest_json(archive, "pack.manifest.json") {
        Ok(Some(manifest)) => return Ok(manifest),
        Ok(None) => {}
        Err(err) => {
            return Err(anyhow::anyhow!(
                "failed to decode pack.manifest.json in {}: {err}",
                path.display()
            ));
        }
    }
    Err(anyhow::anyhow!(
        "pack manifest not found in archive {} (expected manifest.cbor or pack.manifest.json)",
        path.display()
    ))
}

fn read_manifest_cbor(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> anyhow::Result<Option<PackManifest>> {
    let mut file = match archive.by_name("manifest.cbor") {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes)?;
    let value: CborValue = serde_cbor::from_slice(&bytes)?;
    match decode_manifest_lenient(&value) {
        Ok(manifest) => Ok(Some(manifest)),
        Err(decode_err) => {
            if let Some(path) = find_manifest_string_type_mismatch(&value) {
                return Err(anyhow::anyhow!(
                    "invalid type at {} (expected string)",
                    path
                ));
            }
            Err(anyhow::anyhow!(
                "manifest.cbor uses symbol table encoding but could not be decoded: {decode_err}"
            ))
        }
    }
}

fn read_manifest_json(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> anyhow::Result<Option<PackManifest>> {
    let mut file = match archive.by_name(name) {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut contents = String::new();
    std::io::Read::read_to_string(&mut file, &mut contents)?;
    let manifest: PackManifest = serde_json::from_str(&contents)?;
    Ok(Some(manifest))
}

fn find_manifest_string_type_mismatch(value: &CborValue) -> Option<String> {
    let CborValue::Map(map) = value else {
        return None;
    };
    let symbols = symbols_map(map);

    if let Some(pack_id) = map_get(map, "pack_id")
        && !value_is_string_or_symbol(pack_id, symbols, "pack_ids")
    {
        return Some("pack_id".to_string());
    }

    if let Some(meta) = map_get(map, "meta") {
        let CborValue::Map(meta_map) = meta else {
            return Some("meta".to_string());
        };
        if let Some(pack_id) = map_get(meta_map, "pack_id")
            && !value_is_string_or_symbol(pack_id, symbols, "pack_ids")
        {
            return Some("meta.pack_id".to_string());
        }
        if let Some(entry_flows) = map_get(meta_map, "entry_flows") {
            let CborValue::Array(values) = entry_flows else {
                return Some("meta.entry_flows".to_string());
            };
            for (idx, value) in values.iter().enumerate() {
                if !value_is_string_or_symbol(value, symbols, "flow_ids") {
                    return Some(format!("meta.entry_flows[{idx}]"));
                }
            }
        }
    }

    if let Some(flows) = map_get(map, "flows") {
        let CborValue::Array(values) = flows else {
            return Some("flows".to_string());
        };
        for (idx, value) in values.iter().enumerate() {
            let CborValue::Map(flow) = value else {
                return Some(format!("flows[{idx}]"));
            };
            if let Some(id) = map_get(flow, "id")
                && !value_is_string_or_symbol(id, symbols, "flow_ids")
            {
                return Some(format!("flows[{idx}].id"));
            }
            if let Some(entrypoints) = map_get(flow, "entrypoints") {
                let CborValue::Array(values) = entrypoints else {
                    return Some(format!("flows[{idx}].entrypoints"));
                };
                for (jdx, value) in values.iter().enumerate() {
                    if !value_is_string_or_symbol(value, symbols, "entrypoints") {
                        return Some(format!("flows[{idx}].entrypoints[{jdx}]"));
                    }
                }
            }
        }
    }

    None
}

fn map_get<'a>(
    map: &'a std::collections::BTreeMap<CborValue, CborValue>,
    key: &str,
) -> Option<&'a CborValue> {
    map.iter().find_map(|(k, v)| match k {
        CborValue::Text(text) if text == key => Some(v),
        _ => None,
    })
}

fn symbols_map(
    map: &std::collections::BTreeMap<CborValue, CborValue>,
) -> Option<&std::collections::BTreeMap<CborValue, CborValue>> {
    let symbols = map_get(map, "symbols")?;
    match symbols {
        CborValue::Map(map) => Some(map),
        _ => None,
    }
}

fn value_is_string_or_symbol(
    value: &CborValue,
    symbols: Option<&std::collections::BTreeMap<CborValue, CborValue>>,
    symbol_key: &str,
) -> bool {
    if matches!(value, CborValue::Text(_)) {
        return true;
    }
    let CborValue::Integer(idx) = value else {
        return false;
    };
    let symbols = match symbols {
        Some(symbols) => symbols,
        None => return true,
    };
    let Some(CborValue::Array(values)) = map_get(symbols, symbol_key)
        .or_else(|| map_get(symbols, symbol_key.strip_suffix('s').unwrap_or(symbol_key)))
    else {
        return true;
    };
    let idx = match usize::try_from(*idx) {
        Ok(idx) => idx,
        Err(_) => return true,
    };
    matches!(values.get(idx), Some(CborValue::Text(_)))
}

fn decode_manifest_lenient(value: &CborValue) -> anyhow::Result<PackManifest> {
    let CborValue::Map(map) = value else {
        return Err(anyhow::anyhow!("manifest is not a map"));
    };
    let symbols = symbols_map(map);

    let (meta_pack_id, meta_entry_flows) = if let Some(meta) = map_get(map, "meta") {
        let CborValue::Map(meta_map) = meta else {
            return Err(anyhow::anyhow!("meta is not a map"));
        };
        let pack_id = resolve_string_symbol(map_get(meta_map, "pack_id"), symbols, "pack_ids")?;
        let entry_flows = resolve_string_array(
            map_get(meta_map, "entry_flows"),
            symbols,
            "flow_ids",
            Some("entrypoints"),
        )?;
        (pack_id, entry_flows)
    } else {
        (None, Vec::new())
    };

    let pack_id = resolve_string_symbol(map_get(map, "pack_id"), symbols, "pack_ids")?
        .or(meta_pack_id)
        .ok_or_else(|| anyhow::anyhow!("pack_id missing"))?;

    let mut flows = Vec::new();
    if let Some(flows_value) = map_get(map, "flows") {
        let CborValue::Array(values) = flows_value else {
            return Err(anyhow::anyhow!("flows is not an array"));
        };
        for (idx, value) in values.iter().enumerate() {
            let CborValue::Map(flow) = value else {
                return Err(anyhow::anyhow!("flows[{idx}] is not a map"));
            };
            let id = resolve_string_symbol(map_get(flow, "id"), symbols, "flow_ids")?
                .ok_or_else(|| anyhow::anyhow!("flows[{idx}].id missing"))?;
            let entrypoints =
                resolve_string_array(map_get(flow, "entrypoints"), symbols, "entrypoints", None)?;
            flows.push(PackFlow { id, entrypoints });
        }
    }

    Ok(PackManifest {
        meta: Some(PackMeta {
            pack_id,
            entry_flows: meta_entry_flows,
        }),
        pack_id: None,
        flows,
    })
}

fn resolve_string_symbol(
    value: Option<&CborValue>,
    symbols: Option<&std::collections::BTreeMap<CborValue, CborValue>>,
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
        _ => Err(anyhow::anyhow!("expected string or symbol index")),
    }
}

fn symbol_array<'a>(
    symbols: &'a std::collections::BTreeMap<CborValue, CborValue>,
    key: &'a str,
) -> Option<&'a Vec<CborValue>> {
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

fn resolve_string_array(
    value: Option<&CborValue>,
    symbols: Option<&std::collections::BTreeMap<CborValue, CborValue>>,
    symbol_key: &str,
    fallback_key: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let CborValue::Array(values) = value else {
        return Err(anyhow::anyhow!("expected array"));
    };
    let mut out = Vec::new();
    for (idx, value) in values.iter().enumerate() {
        match resolve_string_symbol(Some(value), symbols, symbol_key) {
            Ok(Some(value)) => out.push(value),
            Ok(None) => {}
            Err(err) => {
                if let Some(fallback_key) = fallback_key
                    && let Ok(Some(value)) =
                        resolve_string_symbol(Some(value), symbols, fallback_key)
                {
                    out.push(value);
                    continue;
                }
                return Err(anyhow::anyhow!("{err} at index {idx}"));
            }
        }
    }
    Ok(out)
}

fn missing_cbor_error(path: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "ERROR: demo packs must be CBOR-only (.gtpack must contain manifest.cbor). Rebuild the pack with greentic-pack build (do not use --dev). Missing in {}",
        path.display()
    )
}

pub(crate) fn domain_name(domain: Domain) -> &'static str {
    match domain {
        Domain::Messaging => "messaging",
        Domain::Events => "events",
        Domain::Secrets => "secrets",
    }
}
