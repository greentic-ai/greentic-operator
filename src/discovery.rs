use std::path::{Path, PathBuf};

use serde::Serialize;
use zip::result::ZipError;

use crate::domains::{self, Domain};
use crate::runtime_state::write_json;

#[derive(Clone, Debug, Serialize)]
pub struct DiscoveryResult {
    pub domains: DetectedDomains,
    pub providers: Vec<DetectedProvider>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DetectedDomains {
    pub messaging: bool,
    pub events: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct DetectedProvider {
    pub provider_id: String,
    pub domain: String,
    pub pack_path: PathBuf,
    pub id_source: ProviderIdSource,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderIdSource {
    Manifest,
    Filename,
}

pub fn discover(root: &Path) -> anyhow::Result<DiscoveryResult> {
    let mut providers = Vec::new();
    for domain in [Domain::Messaging, Domain::Events] {
        let cfg = domains::config(domain);
        let providers_dir = root.join(cfg.providers_dir);
        if !providers_dir.exists() {
            continue;
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
            let (provider_id, id_source) = match read_pack_id_from_manifest(&path)? {
                Some(pack_id) => (pack_id, ProviderIdSource::Manifest),
                None => {
                    let stem = path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default()
                        .to_string();
                    (stem, ProviderIdSource::Filename)
                }
            };
            providers.push(DetectedProvider {
                provider_id,
                domain: domains::domain_name(domain).to_string(),
                pack_path: path,
                id_source,
            });
        }
    }
    providers.sort_by(|a, b| a.pack_path.cmp(&b.pack_path));
    let domains = DetectedDomains {
        messaging: providers
            .iter()
            .any(|provider| provider.domain == "messaging"),
        events: providers.iter().any(|provider| provider.domain == "events"),
    };
    Ok(DiscoveryResult { domains, providers })
}

pub fn persist(root: &Path, tenant: &str, discovery: &DiscoveryResult) -> anyhow::Result<()> {
    let runtime_root = root.join("state").join("runtime").join(tenant);
    let domains_path = runtime_root.join("detected_domains.json");
    let providers_path = runtime_root.join("detected_providers.json");
    write_json(&domains_path, &discovery.domains)?;
    write_json(&providers_path, &discovery.providers)?;
    Ok(())
}

fn read_pack_id_from_manifest(path: &Path) -> anyhow::Result<Option<String>> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let manifest = match archive.by_name("pack.manifest.json") {
        Ok(mut file) => {
            let mut contents = String::new();
            std::io::Read::read_to_string(&mut file, &mut contents)?;
            Some(contents)
        }
        Err(ZipError::FileNotFound) => None,
        Err(err) => return Err(err.into()),
    };
    let Some(manifest) = manifest else {
        return Ok(None);
    };
    let parsed: domains::PackManifestForDiscovery = serde_json::from_str(&manifest)?;
    if let Some(meta) = parsed.meta {
        return Ok(Some(meta.pack_id));
    }
    if let Some(name) = parsed.name {
        return Ok(Some(name));
    }
    Ok(None)
}
