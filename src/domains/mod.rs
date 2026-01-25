use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use zip::result::ZipError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    let manifest: PackManifest = serde_cbor::from_slice(&bytes)?;
    Ok(Some(manifest))
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
