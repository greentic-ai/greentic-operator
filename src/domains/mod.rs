use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
            .ok_or_else(|| anyhow::anyhow!("pack manifest missing meta"))?;
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
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PackManifest {
    #[serde(default)]
    meta: Option<PackMeta>,
    #[serde(default)]
    name: Option<String>,
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
    let mut manifest = archive.by_name("pack.manifest.json")?;
    let mut contents = String::new();
    std::io::Read::read_to_string(&mut manifest, &mut contents)?;
    let manifest: PackManifest = serde_json::from_str(&contents)?;
    let pack_id = if let Some(meta) = manifest.meta.as_ref() {
        meta.pack_id.clone()
    } else {
        manifest
            .name
            .clone()
            .ok_or_else(|| anyhow::anyhow!("pack.manifest.json missing pack id/name"))?
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
        name: None,
        flows: Vec::new(),
    })
}

pub(crate) fn domain_name(domain: Domain) -> &'static str {
    match domain {
        Domain::Messaging => "messaging",
        Domain::Events => "events",
        Domain::Secrets => "secrets",
    }
}
