use std::path::{Path, PathBuf};

use crate::dev_mode::{DevProfile, DevSettingsResolved, profile_dir};

pub struct ResolveCtx {
    pub config_dir: PathBuf,
    pub dev: Option<DevSettingsResolved>,
    pub explicit_path: Option<PathBuf>,
}

pub fn resolve_binary(name: &str, ctx: &ResolveCtx) -> anyhow::Result<PathBuf> {
    if let Some(explicit) = ctx.explicit_path.as_ref() {
        let resolved = resolve_relative(&ctx.config_dir, explicit);
        if resolved.exists() {
            return Ok(resolved);
        }
        return Err(anyhow::anyhow!(
            "explicit binary path not found: {}",
            resolved.display()
        ));
    }

    let mut tried = Vec::new();

    if let Some(dev) = ctx.dev.as_ref()
        && let Some(repo) = dev.repo_map.get(name).cloned()
    {
        if !dev.root.exists() {
            return Err(anyhow::anyhow!(
                "dev root not found: {}",
                dev.root.display()
            ));
        }
        let base_repo = dev.root.join(&repo);
        let target_base = dev
            .target_dir
            .clone()
            .unwrap_or_else(|| base_repo.join("target"));

        let candidate = target_base
            .join(profile_dir(dev.profile))
            .join(binary_name(name));
        if candidate.exists() {
            return Ok(candidate);
        }
        tried.push(candidate);

        if dev.profile == DevProfile::Debug {
            let fallback = target_base.join("release").join(binary_name(name));
            if fallback.exists() {
                return Ok(fallback);
            }
            tried.push(fallback);
        }

        let mut message = format!("dev binary not found: {name}");
        message.push_str("\nTried:");
        for path in &tried {
            message.push_str(&format!("\n  - {}", path.display()));
        }
        message.push_str(&format!(
            "\nSuggestions:\n  - cargo build -p {repo}\n  - update dev.repo_map for {name}\n  - set binaries.{name} in greentic.yaml"
        ));
        return Err(anyhow::anyhow!(message));
    }

    let local_candidates = vec![
        ctx.config_dir.join("bin").join(binary_name(name)),
        ctx.config_dir
            .join("target")
            .join("debug")
            .join(binary_name(name)),
        ctx.config_dir
            .join("target")
            .join("release")
            .join(binary_name(name)),
    ];
    for candidate in local_candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
        tried.push(candidate);
    }

    if let Some(path) = find_on_path(name) {
        return Ok(path);
    }

    let mut message = format!("binary not found: {name}");
    if !tried.is_empty() {
        message.push_str("\nTried:");
        for path in &tried {
            message.push_str(&format!("\n  - {}", path.display()));
        }
    }
    if let Some(dev) = ctx.dev.as_ref() {
        let repo = dev
            .repo_map
            .get(name)
            .cloned()
            .unwrap_or_else(|| infer_repo(name));
        message.push_str(&format!(
            "\nSuggestions:\n  - cargo build -p {repo}\n  - set dev.repo_map for {name}\n  - set binaries.{name} in greentic.yaml"
        ));
    }
    Err(anyhow::anyhow!(message))
}

fn infer_repo(name: &str) -> String {
    if name.starts_with("greentic-") {
        return name.to_string();
    }
    name.to_string()
}

fn resolve_relative(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        if name.ends_with(".exe") {
            name.to_string()
        } else {
            format!("{name}.exe")
        }
    } else {
        name.to_string()
    }
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary_name(binary));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
