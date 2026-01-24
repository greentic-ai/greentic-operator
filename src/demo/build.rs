use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub out_dir: PathBuf,
    pub tenant: Option<String>,
    pub team: Option<String>,
    pub allow_pack_dirs: bool,
    pub only_used_providers: bool,
    pub run_doctor: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct ResolvedManifest {
    version: String,
    tenant: String,
    team: Option<String>,
    project_root: String,
    providers: BTreeMap<String, Vec<String>>,
    packs: Vec<String>,
    env_passthrough: Vec<String>,
    policy: serde_yaml_bw::Value,
}

pub fn build_bundle(
    project_root: &Path,
    options: BuildOptions,
    pack_command: Option<&Path>,
) -> anyhow::Result<()> {
    if options.run_doctor && std::env::var("GREENTIC_OPERATOR_SKIP_DOCTOR").is_err() {
        let pack_command = pack_command
            .ok_or_else(|| anyhow::anyhow!("greentic-pack command is required for demo doctor"))?;
        crate::doctor::run_doctor(
            project_root,
            crate::doctor::DoctorScope::All,
            crate::doctor::DoctorOptions {
                tenant: options.tenant.clone(),
                team: options.team.clone(),
                strict: false,
                validator_packs: Vec::new(),
            },
            pack_command,
        )?;
    }

    let resolved_dir = project_root.join("state").join("resolved");
    if !resolved_dir.exists() {
        return Err(anyhow::anyhow!(
            "Resolved manifests not found. Run `greentic-operator dev sync` first."
        ));
    }

    let manifests = select_manifests(
        &resolved_dir,
        options.tenant.as_deref(),
        options.team.as_deref(),
    )?;
    if manifests.is_empty() {
        return Err(anyhow::anyhow!(
            "No resolved manifests found for selection."
        ));
    }

    let bundle_root = options.out_dir;
    std::fs::create_dir_all(&bundle_root)?;
    std::fs::create_dir_all(bundle_root.join("providers"))?;
    std::fs::create_dir_all(bundle_root.join("packs"))?;
    std::fs::create_dir_all(bundle_root.join("tenants"))?;
    std::fs::create_dir_all(bundle_root.join("resolved"))?;
    std::fs::create_dir_all(bundle_root.join("state"))?;

    let mut used_provider_paths = BTreeSet::new();
    let mut loaded_manifests = Vec::new();
    for manifest_path in &manifests {
        let manifest = load_manifest(manifest_path)?;
        for packs in manifest.providers.values() {
            for pack in packs {
                used_provider_paths.insert(pack.clone());
            }
        }
        loaded_manifests.push((manifest_path.clone(), manifest));
    }

    if options.only_used_providers {
        for provider_path in &used_provider_paths {
            let from = project_root.join(provider_path);
            let to = bundle_root.join(provider_path);
            copy_file(&from, &to)?;
        }
    } else {
        copy_dir(
            project_root.join("providers"),
            bundle_root.join("providers"),
        )?;
    }

    let mut tenants_to_copy = BTreeSet::new();
    for (manifest_path, mut manifest) in loaded_manifests {
        tenants_to_copy.insert(manifest.tenant.clone());

        let pack_paths = manifest.packs.clone();
        for pack in pack_paths {
            let pack_path = project_root.join(&pack);
            if pack.ends_with(".gtpack") {
                copy_file(&pack_path, &bundle_root.join(&pack))?;
            } else {
                if !options.allow_pack_dirs {
                    return Err(anyhow::anyhow!(
                        "Pack directory not allowed in demo bundle: {} (use --allow-pack-dirs)",
                        pack
                    ));
                }
                eprintln!(
                    "Warning: copying pack directory into demo bundle (not portable): {}",
                    pack
                );
                copy_dir(pack_path, bundle_root.join(&pack))?;
            }
        }

        manifest.project_root = "./".to_string();
        let filename = manifest_path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid manifest filename"))?;
        let out_path = bundle_root.join("resolved").join(filename);
        write_manifest(&out_path, &manifest)?;
    }

    for tenant in tenants_to_copy {
        let tenant_path = project_root.join("tenants").join(&tenant);
        if tenant_path.exists() {
            copy_dir(tenant_path, bundle_root.join("tenants").join(&tenant))?;
        }
    }

    let demo_meta = bundle_root.join("greentic.demo.yaml");
    write_demo_metadata(&demo_meta)?;

    Ok(())
}

fn select_manifests(
    resolved_dir: &Path,
    tenant: Option<&str>,
    team: Option<&str>,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut manifests = Vec::new();
    if let Some(tenant) = tenant {
        let filename = match team {
            Some(team) => format!("{tenant}.{team}.yaml"),
            None => format!("{tenant}.yaml"),
        };
        let path = resolved_dir.join(filename);
        if path.exists() {
            manifests.push(path);
        }
        return Ok(manifests);
    }

    for entry in std::fs::read_dir(resolved_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("yaml") {
                manifests.push(path);
            }
        }
    }
    manifests.sort();
    Ok(manifests)
}

fn load_manifest(path: &Path) -> anyhow::Result<ResolvedManifest> {
    let contents = std::fs::read_to_string(path)?;
    let manifest: ResolvedManifest = serde_yaml_bw::from_str(&contents)?;
    Ok(manifest)
}

fn write_manifest(path: &Path, manifest: &ResolvedManifest) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let yaml = serde_yaml_bw::to_string(manifest)?;
    std::fs::write(path, yaml)?;
    Ok(())
}

fn write_demo_metadata(path: &Path) -> anyhow::Result<()> {
    let contents = "version: \"1\"\nproject_root: \"./\"\n";
    std::fs::write(path, contents)?;
    Ok(())
}

fn copy_file(from: &Path, to: &Path) -> anyhow::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to)?;
    Ok(())
}

fn copy_dir(from: PathBuf, to: PathBuf) -> anyhow::Result<()> {
    if !from.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&to)?;
    for entry in std::fs::read_dir(&from)? {
        let entry = entry?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(path, target)?;
        } else {
            copy_file(&path, &target)?;
        }
    }
    Ok(())
}
