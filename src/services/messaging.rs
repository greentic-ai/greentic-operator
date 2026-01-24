use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::runner::{ProcessStatus, ServiceState, log_path, pid_path};

#[derive(Debug, Deserialize)]
struct ResolvedManifest {
    tenant: String,
    team: Option<String>,
    project_root: Option<String>,
    providers: Option<BTreeMap<String, Vec<String>>>,
    packs: Vec<String>,
}

pub fn start_messaging(
    root: &Path,
    tenant: &str,
    team: Option<&str>,
    nats_url: Option<&str>,
) -> anyhow::Result<ServiceState> {
    start_messaging_with_command(root, tenant, team, nats_url, "greentic-messaging")
}

pub fn start_messaging_with_command(
    root: &Path,
    tenant: &str,
    team: Option<&str>,
    nats_url: Option<&str>,
    command: &str,
) -> anyhow::Result<ServiceState> {
    let manifest_path = resolved_manifest_path(root, tenant, team);
    let name = messaging_name(tenant, team);
    start_messaging_from_manifest(root, &manifest_path, &name, nats_url, command)
}

pub fn start_messaging_from_manifest(
    root: &Path,
    manifest_path: &Path,
    name: &str,
    nats_url: Option<&str>,
    command: &str,
) -> anyhow::Result<ServiceState> {
    let manifest = load_manifest(manifest_path)?;

    let mut envs = Vec::new();
    if let Some(url) = nats_url {
        envs.push(("NATS_URL", url.to_string()));
    }
    envs.push((
        "CARGO_TARGET_DIR",
        root.join("state")
            .join("cargo-target")
            .to_string_lossy()
            .to_string(),
    ));

    let mut args = vec![
        "serve".to_string(),
        "--tenant".to_string(),
        manifest.tenant.clone(),
    ];
    if let Some(team) = &manifest.team {
        args.push("--team".to_string());
        args.push(team.clone());
    }
    args.push("--no-default-packs".to_string());
    for pack in messaging_adapter_packs(root, &manifest) {
        args.push("--pack".to_string());
        args.push(pack);
    }
    if let Some(packs_root) = packs_root_path(root, &manifest) {
        args.push("--packs-root".to_string());
        args.push(packs_root.to_string_lossy().to_string());
    }
    args.push("pack".to_string());

    let pid = pid_path(root, name);
    let log = log_path(root, name);

    let cwd = messaging_cwd(command, root);
    super::runner::start_process(command, &args, &envs, &pid, &log, cwd.as_deref())
}

pub fn stop_messaging(
    root: &Path,
    tenant: &str,
    team: Option<&str>,
) -> anyhow::Result<ServiceState> {
    let name = messaging_name(tenant, team);
    let pid = pid_path(root, &name);
    super::runner::stop_process(&pid)
}

pub fn messaging_status(
    root: &Path,
    tenant: &str,
    team: Option<&str>,
) -> anyhow::Result<ProcessStatus> {
    let name = messaging_name(tenant, team);
    let pid = pid_path(root, &name);
    super::runner::process_status(&pid)
}

pub fn tail_messaging_logs(root: &Path, tenant: &str, team: Option<&str>) -> anyhow::Result<()> {
    let name = messaging_name(tenant, team);
    let log = log_path(root, &name);
    super::runner::tail_log(&log)
}

pub fn messaging_name(tenant: &str, team: Option<&str>) -> String {
    match team {
        Some(team) => format!("messaging-{tenant}-{team}"),
        None => format!("messaging-{tenant}"),
    }
}

fn resolved_manifest_path(root: &Path, tenant: &str, team: Option<&str>) -> std::path::PathBuf {
    let filename = match team {
        Some(team) => format!("{tenant}.{team}.yaml"),
        None => format!("{tenant}.yaml"),
    };
    root.join("state").join("resolved").join(filename)
}

fn load_manifest(path: &Path) -> anyhow::Result<ResolvedManifest> {
    let contents = std::fs::read_to_string(path)?;
    let manifest: ResolvedManifest = serde_yaml_bw::from_str(&contents)?;
    Ok(manifest)
}

fn messaging_adapter_packs(root: &Path, manifest: &ResolvedManifest) -> Vec<String> {
    let mut packs = Vec::new();
    let base = manifest_base(root, manifest);
    if let Some(providers) = manifest.providers.as_ref()
        && let Some(adapter_packs) = providers.get("messaging")
    {
        packs.extend(
            adapter_packs
                .iter()
                .map(|pack| absolutize_pack_path(&base, pack)),
        );
    }
    packs.extend(
        manifest
            .packs
            .iter()
            .map(|pack| absolutize_pack_path(&base, pack)),
    );
    packs
}

fn manifest_base(root: &Path, manifest: &ResolvedManifest) -> std::path::PathBuf {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if let Some(project_root) = manifest.project_root.as_ref() {
        let project_root = Path::new(project_root);
        if project_root.is_absolute() {
            return project_root.to_path_buf();
        }
        return root.join(project_root);
    }
    root
}

fn absolutize_pack_path(base: &Path, pack: &str) -> String {
    let path = Path::new(pack);
    if path.is_absolute() {
        return pack.to_string();
    }
    base.join(path).to_string_lossy().to_string()
}

fn packs_root_path(root: &Path, manifest: &ResolvedManifest) -> Option<std::path::PathBuf> {
    let base = manifest_base(root, manifest);
    Some(base.join("packs"))
}

fn messaging_cwd(command: &str, fallback: &Path) -> Option<std::path::PathBuf> {
    let command_path = Path::new(command);
    if !command_path.is_absolute() {
        return Some(fallback.to_path_buf());
    }
    let parent = command_path.parent()?;
    let profile = parent.file_name()?.to_str()?;
    if profile != "debug" && profile != "release" {
        return Some(fallback.to_path_buf());
    }
    let target = parent.parent()?;
    if target.file_name()?.to_str()? != "target" {
        return Some(fallback.to_path_buf());
    }
    Some(target.parent()?.to_path_buf())
}
