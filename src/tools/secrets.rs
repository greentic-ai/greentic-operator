use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::state_layout;

pub fn resolve_env() -> String {
    std::env::var("GREENTIC_ENV").unwrap_or_else(|_| "dev".to_string())
}

pub fn run_init(
    root: &Path,
    secrets_bin: Option<&Path>,
    env: &str,
    tenant: &str,
    team: Option<&str>,
    pack: &Path,
    interactive: bool,
) -> anyhow::Result<ExitStatus> {
    let bin = resolve_secrets_bin(secrets_bin)?;
    let args = build_init_args(env, tenant, team, pack, interactive);
    let log_path = state_layout::secrets_log_path(root, "init")?;
    run_with_logging(&bin, &args, &log_path)
}

pub fn run_set(
    root: &Path,
    secrets_bin: Option<&Path>,
    env: &str,
    tenant: &str,
    team: Option<&str>,
    key: &str,
    value: Option<&str>,
) -> anyhow::Result<ExitStatus> {
    let bin = resolve_secrets_bin(secrets_bin)?;
    let args = build_set_args(env, tenant, team, key, value);
    let log_path = state_layout::secrets_log_path(root, "set")?;
    run_with_logging(&bin, &args, &log_path)
}

pub fn run_get(
    root: &Path,
    secrets_bin: Option<&Path>,
    env: &str,
    tenant: &str,
    team: Option<&str>,
    key: &str,
) -> anyhow::Result<ExitStatus> {
    let bin = resolve_secrets_bin(secrets_bin)?;
    let args = build_get_args(env, tenant, team, key);
    let log_path = state_layout::secrets_log_path(root, "get")?;
    run_with_logging(&bin, &args, &log_path)
}

pub fn run_list(
    root: &Path,
    secrets_bin: Option<&Path>,
    env: &str,
    tenant: &str,
    team: Option<&str>,
) -> anyhow::Result<ExitStatus> {
    let bin = resolve_secrets_bin(secrets_bin)?;
    let args = build_list_args(env, tenant, team);
    let log_path = state_layout::secrets_log_path(root, "list")?;
    run_with_logging(&bin, &args, &log_path)
}

pub fn run_delete(
    root: &Path,
    secrets_bin: Option<&Path>,
    env: &str,
    tenant: &str,
    team: Option<&str>,
    key: &str,
) -> anyhow::Result<ExitStatus> {
    let bin = resolve_secrets_bin(secrets_bin)?;
    let args = build_delete_args(env, tenant, team, key);
    let log_path = state_layout::secrets_log_path(root, "delete")?;
    run_with_logging(&bin, &args, &log_path)
}

pub(crate) fn build_init_args(
    env: &str,
    tenant: &str,
    team: Option<&str>,
    pack: &Path,
    interactive: bool,
) -> Vec<String> {
    let mut args = vec![
        "init".to_string(),
        "--env".to_string(),
        env.to_string(),
        "--tenant".to_string(),
        tenant.to_string(),
    ];
    if let Some(team) = team {
        args.push("--team".to_string());
        args.push(team.to_string());
    }
    args.push("--pack".to_string());
    args.push(pack.display().to_string());
    if !interactive {
        args.push("--non-interactive".to_string());
    }
    args
}

pub(crate) fn build_set_args(
    env: &str,
    tenant: &str,
    team: Option<&str>,
    key: &str,
    value: Option<&str>,
) -> Vec<String> {
    let mut args = build_base_args(env, tenant, team);
    args.insert(0, "set".to_string());
    args.push(key.to_string());
    if let Some(value) = value {
        args.push(value.to_string());
    }
    args
}

pub(crate) fn build_get_args(
    env: &str,
    tenant: &str,
    team: Option<&str>,
    key: &str,
) -> Vec<String> {
    let mut args = build_base_args(env, tenant, team);
    args.insert(0, "get".to_string());
    args.push(key.to_string());
    args
}

pub(crate) fn build_list_args(env: &str, tenant: &str, team: Option<&str>) -> Vec<String> {
    let mut args = build_base_args(env, tenant, team);
    args.insert(0, "list".to_string());
    args
}

pub(crate) fn build_delete_args(
    env: &str,
    tenant: &str,
    team: Option<&str>,
    key: &str,
) -> Vec<String> {
    let mut args = build_base_args(env, tenant, team);
    args.insert(0, "delete".to_string());
    args.push(key.to_string());
    args
}

fn build_base_args(env: &str, tenant: &str, team: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--env".to_string(),
        env.to_string(),
        "--tenant".to_string(),
        tenant.to_string(),
    ];
    if let Some(team) = team {
        args.push("--team".to_string());
        args.push(team.to_string());
    }
    args
}

fn resolve_secrets_bin(override_path: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(path) = override_path {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        return Err(anyhow::anyhow!(
            "greentic-secrets binary not found at {}",
            path.display()
        ));
    }
    if let Some(path) = find_on_path("greentic-secrets") {
        return Ok(path);
    }
    Err(anyhow::anyhow!(
        "greentic-secrets binary not found. Install it or pass --secrets-bin."
    ))
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
        if cfg!(windows) {
            let exe = dir.join(format!("{binary}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

fn run_with_logging(bin: &Path, args: &[String], log_path: &Path) -> anyhow::Result<ExitStatus> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let log_out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let log_err = log_out.try_clone()?;

    let mut child = Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture stdout for {}", bin.display()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture stderr for {}", bin.display()))?;

    let out_handle = std::thread::spawn(move || pipe_output(stdout, log_out, false));
    let err_handle = std::thread::spawn(move || pipe_output(stderr, log_err, true));

    let status = child.wait()?;
    out_handle
        .join()
        .map_err(|_| anyhow::anyhow!("stdout thread panicked"))??;
    err_handle
        .join()
        .map_err(|_| anyhow::anyhow!("stderr thread panicked"))??;

    Ok(status)
}

fn pipe_output(mut reader: impl Read, mut log: File, is_err: bool) -> anyhow::Result<()> {
    let mut buf = [0u8; 4096];
    let mut stream: Box<dyn Write> = if is_err {
        Box::new(std::io::stderr())
    } else {
        Box::new(std::io::stdout())
    };

    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        stream.write_all(&buf[..read])?;
        stream.flush()?;
        log.write_all(&buf[..read])?;
        log.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_init_args_with_team() {
        let args = build_init_args(
            "dev",
            "tenant1",
            Some("team1"),
            Path::new("pack.gtpack"),
            false,
        );
        assert_eq!(
            args,
            vec![
                "init",
                "--env",
                "dev",
                "--tenant",
                "tenant1",
                "--team",
                "team1",
                "--pack",
                "pack.gtpack",
                "--non-interactive"
            ]
        );
    }

    #[test]
    fn build_init_args_without_team() {
        let args = build_init_args("dev", "tenant1", None, Path::new("pack.gtpack"), false);
        assert_eq!(
            args,
            vec![
                "init",
                "--env",
                "dev",
                "--tenant",
                "tenant1",
                "--pack",
                "pack.gtpack",
                "--non-interactive"
            ]
        );
    }

    #[test]
    fn build_set_args_with_value() {
        let args = build_set_args("dev", "tenant1", Some("team1"), "KEY", Some("VALUE"));
        assert_eq!(
            args,
            vec![
                "set", "--env", "dev", "--tenant", "tenant1", "--team", "team1", "KEY", "VALUE"
            ]
        );
    }

    #[test]
    fn build_set_args_without_value() {
        let args = build_set_args("dev", "tenant1", None, "KEY", None);
        assert_eq!(
            args,
            vec!["set", "--env", "dev", "--tenant", "tenant1", "KEY"]
        );
    }

    #[test]
    fn build_get_args_with_team() {
        let args = build_get_args("dev", "tenant1", Some("team1"), "KEY");
        assert_eq!(
            args,
            vec![
                "get", "--env", "dev", "--tenant", "tenant1", "--team", "team1", "KEY"
            ]
        );
    }

    #[test]
    fn build_list_args_without_team() {
        let args = build_list_args("dev", "tenant1", None);
        assert_eq!(args, vec!["list", "--env", "dev", "--tenant", "tenant1"]);
    }

    #[test]
    fn build_delete_args_with_team() {
        let args = build_delete_args("dev", "tenant1", Some("team1"), "KEY");
        assert_eq!(
            args,
            vec![
                "delete", "--env", "dev", "--tenant", "tenant1", "--team", "team1", "KEY"
            ]
        );
    }
}
