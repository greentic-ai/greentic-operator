use std::path::{Path, PathBuf};

use std::collections::{BTreeMap, BTreeSet};

use crate::cloudflared::{self, CloudflaredConfig};
use crate::config::DemoConfig;
use crate::dev_mode::DevSettingsResolved;
use crate::runtime_state::{RuntimePaths, write_json};
use crate::services;
use crate::supervisor;

#[allow(clippy::too_many_arguments)]
pub fn demo_up(
    bundle_root: &Path,
    tenant: &str,
    team: Option<&str>,
    nats_url: Option<&str>,
    no_nats: bool,
    messaging_enabled: bool,
    cloudflared: Option<CloudflaredConfig>,
    events_components: Vec<crate::services::ComponentSpec>,
) -> anyhow::Result<()> {
    if let Some(config) = cloudflared {
        let state_dir = bundle_root.join("state");
        let team_id = team.unwrap_or("default");
        let paths = RuntimePaths::new(&state_dir, tenant, team_id);
        let handle = cloudflared::start_quick_tunnel(&paths, &config)?;
        println!("Public URL (service=cloudflared): {}", handle.url);
    }

    let mut resolved_nats_url = nats_url.map(|value| value.to_string());
    let mut nats_started = false;
    if !no_nats && resolved_nats_url.is_none() {
        if let Err(err) = services::start_nats(bundle_root) {
            eprintln!("Warning: failed to start NATS: {err}");
        } else {
            resolved_nats_url = Some(services::nats_url(bundle_root));
            nats_started = true;
        }
    }

    if messaging_enabled {
        crate::services::run_services(bundle_root)?;
    } else {
        println!("messaging: skipped (disabled or no providers)");
    }

    let mut running_components = Vec::new();
    if !events_components.is_empty() && resolved_nats_url.is_some() {
        let envs = build_env_kv(tenant, team, resolved_nats_url.as_deref());
        for component in events_components {
            let state = services::start_component(bundle_root, &component, &envs)?;
            println!("{}: {:?}", component.id, state);
            running_components.push(component);
        }
    } else {
        println!("events: skipped (disabled or no providers)");
    }

    for component in running_components {
        let id = component.id;
        let state = services::stop_component(bundle_root, &id)?;
        println!("{id}: {:?}", state);
    }

    if nats_started {
        let nats = services::stop_nats(bundle_root)?;
        println!("nats: {:?}", nats);
    }

    Ok(())
}

fn build_env_kv(
    tenant: &str,
    team: Option<&str>,
    nats_url: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut envs = Vec::new();
    envs.push(("GREENTIC_TENANT", tenant.to_string()));
    if let Some(team) = team {
        envs.push(("GREENTIC_TEAM", team.to_string()));
    }
    if let Some(nats_url) = nats_url {
        envs.push(("NATS_URL", nats_url.to_string()));
    }
    envs
}

pub fn demo_up_services(
    config_path: &Path,
    config: &DemoConfig,
    dev_settings: Option<DevSettingsResolved>,
    cloudflared: Option<CloudflaredConfig>,
    restart: &BTreeSet<String>,
    provider_options: crate::providers::ProviderSetupOptions,
) -> anyhow::Result<()> {
    let config_dir = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config path has no parent directory"))?;
    let state_dir = config_dir.join("state");
    let tenant = config.tenant.as_str();
    let team = config.team.as_str();
    let paths = RuntimePaths::new(&state_dir, tenant, team);
    let discovery = crate::discovery::discover(config_dir)?;
    crate::discovery::persist(config_dir, tenant, &discovery)?;

    if should_restart(restart, "cloudflared") {
        let _ = supervisor::stop_pidfile(&paths.pid_path("cloudflared"), 2_000);
    }

    let public_base_url = if let Some(cfg) = cloudflared {
        let handle = cloudflared::start_quick_tunnel(&paths, &cfg)?;
        let mut domain_labels = Vec::new();
        if discovery.domains.messaging {
            domain_labels.push("messaging");
        }
        if discovery.domains.events {
            domain_labels.push("events");
        }
        let domain_list = if domain_labels.is_empty() {
            "none".to_string()
        } else {
            domain_labels.join(",")
        };
        println!(
            "Public URL (service=cloudflared domains={domain_list}): {}",
            handle.url
        );
        Some(handle.url)
    } else {
        None
    };

    if should_restart(restart, "nats") {
        let _ = supervisor::stop_pidfile(&paths.pid_path("nats"), 2_000);
    }

    let nats_url = if config.services.nats.enabled {
        if config.services.nats.spawn.enabled {
            let spec = build_service_spec(
                config_dir,
                dev_settings.as_ref(),
                "nats",
                &config.services.nats.spawn.binary,
                &config.services.nats.spawn.args,
                &build_env(tenant, team, None, public_base_url.as_deref()),
            )?;
            let _ = spawn_if_needed(&paths, &spec, restart)?;
        }
        Some(config.services.nats.url.clone())
    } else {
        None
    };

    let events_enabled = config
        .services
        .events
        .enabled
        .is_enabled(discovery.domains.events);
    if events_enabled {
        for component in &config.services.events.components {
            if should_restart(restart, &component.id) {
                let _ = supervisor::stop_pidfile(&paths.pid_path(&component.id), 2_000);
            }
            let spec = build_service_spec(
                config_dir,
                dev_settings.as_ref(),
                &component.id,
                &component.binary,
                &component.args,
                &build_env(
                    tenant,
                    team,
                    nats_url.as_deref(),
                    public_base_url.as_deref(),
                ),
            )?;
            let _ = spawn_if_needed(&paths, &spec, restart)?;
        }
    }

    if should_restart(restart, "gateway") {
        let _ = supervisor::stop_pidfile(&paths.pid_path("gateway"), 2_000);
    }
    let gateway_spec = build_service_spec(
        config_dir,
        dev_settings.as_ref(),
        "gateway",
        &config.services.gateway.binary,
        &config.services.gateway.args,
        &build_env(
            tenant,
            team,
            nats_url.as_deref(),
            public_base_url.as_deref(),
        ),
    )?;
    let _ = spawn_if_needed(&paths, &gateway_spec, restart)?;

    if should_restart(restart, "egress") {
        let _ = supervisor::stop_pidfile(&paths.pid_path("egress"), 2_000);
    }
    let egress_spec = build_service_spec(
        config_dir,
        dev_settings.as_ref(),
        "egress",
        &config.services.egress.binary,
        &config.services.egress.args,
        &build_env(
            tenant,
            team,
            nats_url.as_deref(),
            public_base_url.as_deref(),
        ),
    )?;
    let _ = spawn_if_needed(&paths, &egress_spec, restart)?;

    if config.services.subscriptions.msgraph.enabled {
        if should_restart(restart, "subscriptions") || should_restart(restart, "msgraph") {
            let _ = supervisor::stop_pidfile(&paths.pid_path("subscriptions"), 2_000);
        }
        let mut args = config.services.subscriptions.msgraph.args.clone();
        if !config.services.subscriptions.msgraph.mode.is_empty() {
            args.insert(0, config.services.subscriptions.msgraph.mode.clone());
        }
        let spec = build_service_spec(
            config_dir,
            dev_settings.as_ref(),
            "subscriptions",
            &config.services.subscriptions.msgraph.binary,
            &args,
            &build_env(
                tenant,
                team,
                nats_url.as_deref(),
                public_base_url.as_deref(),
            ),
        )?;
        let _ = spawn_if_needed(&paths, &spec, restart)?;
    }

    let endpoints = DemoEndpoints {
        tenant: tenant.to_string(),
        team: team.to_string(),
        public_base_url,
        nats_url,
        gateway_listen_addr: config.services.gateway.listen_addr.clone(),
        gateway_port: config.services.gateway.port,
    };
    write_json(&paths.runtime_root().join("endpoints.json"), &endpoints)?;

    crate::providers::run_provider_setup(
        config_dir,
        config,
        dev_settings,
        endpoints.public_base_url.as_deref(),
        provider_options,
    )?;
    Ok(())
}

pub fn demo_status_runtime(
    state_dir: &Path,
    tenant: &str,
    team: &str,
    verbose: bool,
) -> anyhow::Result<()> {
    let paths = RuntimePaths::new(state_dir, tenant, team);
    let statuses = supervisor::read_status(&paths)?;
    if statuses.is_empty() {
        println!("none running");
        return Ok(());
    }
    for status in statuses {
        let state = if status.running { "running" } else { "stopped" };
        let pid = status
            .pid
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string());
        if verbose {
            println!(
                "{}: {} (pid={}, log={})",
                status.id.as_str(),
                state,
                pid,
                status.log_path.display()
            );
        } else {
            println!("{}: {} (pid={})", status.id.as_str(), state, pid);
        }
    }
    Ok(())
}

pub fn demo_logs_runtime(
    state_dir: &Path,
    tenant: &str,
    team: &str,
    service: &str,
    tail: bool,
) -> anyhow::Result<()> {
    let paths = RuntimePaths::new(state_dir, tenant, team);
    let tenant_log_path = prepare_tenant_log_path(&paths, service, tenant, team)?;
    let log_path = select_log_path(&paths, service, tenant, &tenant_log_path);
    if tail {
        return services::tail_log(&log_path);
    }
    let lines = read_last_lines(&log_path, 200)?;
    if !lines.is_empty() {
        println!("{lines}");
    }
    Ok(())
}

pub fn demo_down_runtime(
    state_dir: &Path,
    tenant: &str,
    team: &str,
    all: bool,
) -> anyhow::Result<()> {
    let timeout_ms = 2_000;
    if all {
        let pids_root = state_dir.join("pids");
        if !pids_root.exists() {
            println!("No services to stop.");
            return Ok(());
        }
        for entry in std::fs::read_dir(&pids_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            for pidfile in std::fs::read_dir(entry.path())? {
                let pidfile = pidfile?;
                if pidfile.path().extension().and_then(|ext| ext.to_str()) != Some("pid") {
                    continue;
                }
                let _ = supervisor::stop_pidfile(&pidfile.path(), timeout_ms);
            }
        }
        println!("Stopped all services under {}", pids_root.display());
        return Ok(());
    }

    let paths = RuntimePaths::new(state_dir, tenant, team);
    let pids_dir = paths.pids_dir();
    if !pids_dir.exists() {
        println!("No services to stop.");
        return Ok(());
    }
    for entry in std::fs::read_dir(&pids_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("pid") {
            continue;
        }
        supervisor::stop_pidfile(&path, timeout_ms)?;
    }
    Ok(())
}

fn select_log_path(
    paths: &RuntimePaths,
    service: &str,
    tenant: &str,
    tenant_log: &Path,
) -> PathBuf {
    let logs_root = paths.logs_root();
    let mut candidates = Vec::new();
    candidates.push(tenant_log.to_path_buf());
    candidates.push(logs_root.join(format!("{service}.log")));
    candidates.push(logs_root.join(format!("{service}-{tenant}.log")));
    candidates.push(logs_root.join(format!("{service}.{tenant}.log")));
    candidates.push(paths.log_path(service));
    for candidate in &candidates {
        if candidate.exists() {
            return candidate.clone();
        }
    }
    candidates.into_iter().next().unwrap()
}

fn prepare_tenant_log_path(
    paths: &RuntimePaths,
    service: &str,
    tenant: &str,
    team: &str,
) -> anyhow::Result<PathBuf> {
    let tenant_dir = paths.logs_dir().join(format!("{tenant}.{team}"));
    let path = tenant_dir.join(format!("{service}.log"));
    ensure_log_file(&path)?;
    Ok(path)
}

fn ensure_log_file(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        std::fs::File::create(path)?;
    }
    Ok(())
}

fn build_env(
    tenant: &str,
    team: &str,
    nats_url: Option<&str>,
    public_base_url: Option<&str>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("GREENTIC_TENANT".to_string(), tenant.to_string());
    env.insert("GREENTIC_TEAM".to_string(), team.to_string());
    if let Some(url) = nats_url {
        env.insert("NATS_URL".to_string(), url.to_string());
    }
    if let Some(url) = public_base_url {
        env.insert("PUBLIC_BASE_URL".to_string(), url.to_string());
    }
    env
}

fn build_service_spec(
    config_dir: &Path,
    dev_settings: Option<&DevSettingsResolved>,
    service_id: &str,
    binary: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> anyhow::Result<supervisor::ServiceSpec> {
    let explicit = if looks_like_path(binary) {
        let path = Path::new(binary);
        Some(if path.is_absolute() {
            path.to_path_buf()
        } else {
            config_dir.join(path)
        })
    } else {
        None
    };
    let path = crate::bin_resolver::resolve_binary(
        binary,
        &crate::bin_resolver::ResolveCtx {
            config_dir: config_dir.to_path_buf(),
            dev: dev_settings.cloned(),
            explicit_path: explicit,
        },
    )?;
    let mut argv = vec![path.to_string_lossy().to_string()];
    argv.extend(args.iter().cloned());
    Ok(supervisor::ServiceSpec {
        id: supervisor::ServiceId::new(service_id)?,
        argv,
        cwd: None,
        env: env.clone(),
    })
}

fn spawn_if_needed(
    paths: &RuntimePaths,
    spec: &supervisor::ServiceSpec,
    restart: &BTreeSet<String>,
) -> anyhow::Result<Option<supervisor::ServiceHandle>> {
    if should_restart(restart, spec.id.as_str()) {
        let _ = supervisor::stop_service(paths, &spec.id, 2_000);
    }

    let pid_path = paths.pid_path(spec.id.as_str());
    if let Some(pid) = read_pid(&pid_path)?
        && supervisor::is_running(pid)
    {
        println!("{}: already running (pid={pid})", spec.id.as_str());
        return Ok(None);
    }
    let handle = supervisor::spawn_service(paths, spec.clone())?;
    println!("{}: started (pid={})", spec.id.as_str(), handle.pid);
    Ok(Some(handle))
}

fn read_pid(path: &Path) -> anyhow::Result<Option<u32>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)?;
    let trimmed = contents.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed.parse()?))
}

fn looks_like_path(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || Path::new(value).is_absolute()
}

fn should_restart(restart: &BTreeSet<String>, service: &str) -> bool {
    restart.contains("all") || restart.contains(service)
}

#[derive(serde::Serialize)]
struct DemoEndpoints {
    tenant: String,
    team: String,
    public_base_url: Option<String>,
    nats_url: Option<String>,
    gateway_listen_addr: String,
    gateway_port: u16,
}

fn read_last_lines(path: &Path, count: usize) -> anyhow::Result<String> {
    if !path.exists() {
        return Err(anyhow::anyhow!(
            "Log file does not exist: {}",
            path.display()
        ));
    }
    let contents = std::fs::read_to_string(path)?;
    let mut lines: Vec<&str> = contents.lines().collect();
    if lines.len() > count {
        lines = lines.split_off(lines.len() - count);
    }
    Ok(lines.join("\n"))
}
