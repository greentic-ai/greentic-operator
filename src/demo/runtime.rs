use std::env;
use std::path::{Path, PathBuf};

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context;

use crate::cloudflared::{self, CloudflaredConfig};
use crate::config::DemoConfig;
use crate::dev_mode::DevSettingsResolved;
use crate::operator_log;
use crate::runtime_state::{
    RuntimePaths, ServiceEntry, ServiceManifest, persist_service_manifest, read_service_manifest,
    remove_service_manifest, write_json,
};
use crate::services;
use crate::supervisor;

struct ServiceTracker<'a> {
    paths: &'a RuntimePaths,
    manifest: ServiceManifest,
}

struct ServiceSummary {
    id: String,
    pid: Option<u32>,
    details: Vec<String>,
}

impl ServiceSummary {
    fn new(id: impl Into<String>, pid: Option<u32>) -> Self {
        Self {
            id: id.into(),
            pid,
            details: Vec::new(),
        }
    }

    fn with_details(id: impl Into<String>, pid: Option<u32>, details: Vec<String>) -> Self {
        Self {
            id: id.into(),
            pid,
            details,
        }
    }

    fn add_detail(&mut self, detail: impl Into<String>) {
        self.details.push(detail.into());
    }

    fn describe(&self) -> String {
        let pid_str = self
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "-".to_string());
        if self.details.is_empty() {
            format!("{} (pid={})", self.id, pid_str)
        } else {
            format!(
                "{} (pid={}) [{}]",
                self.id,
                pid_str,
                self.details.join(" | ")
            )
        }
    }
}
impl<'a> ServiceTracker<'a> {
    fn new(paths: &'a RuntimePaths, log_dir: Option<&Path>) -> anyhow::Result<Self> {
        remove_service_manifest(paths)?;
        let mut manifest = ServiceManifest::default();
        if let Some(dir) = log_dir {
            manifest.log_dir = Some(dir.display().to_string());
        }
        persist_service_manifest(paths, &manifest)?;
        Ok(Self { paths, manifest })
    }

    fn record(&mut self, entry: ServiceEntry) -> anyhow::Result<()> {
        self.manifest.services.push(entry);
        persist_service_manifest(self.paths, &self.manifest)
    }

    fn record_with_log(
        &mut self,
        id: impl Into<String>,
        kind: impl Into<String>,
        log_path: Option<&Path>,
    ) -> anyhow::Result<()> {
        let entry = ServiceEntry::new(id, kind, log_path);
        self.record(entry)
    }
}

fn spawn_supervised_service(
    service_id: &str,
    kind: &str,
    spec: &supervisor::ServiceSpec,
    log_dir: &Path,
    paths: &RuntimePaths,
    restart: &BTreeSet<String>,
    tracker: &mut ServiceTracker,
) -> anyhow::Result<ServiceSummary> {
    let log_path = operator_log::reserve_service_log(log_dir, service_id)?;
    let handle = spawn_if_needed(paths, spec, restart, Some(log_path.clone()))?;
    let pid = if let Some(handle) = &handle {
        Some(handle.pid)
    } else {
        read_pid(&paths.pid_path(service_id))?
    };
    let actual_log = handle
        .as_ref()
        .map(|handle| handle.log_path.clone())
        .unwrap_or(log_path.clone());
    tracker.record_with_log(service_id, kind, Some(&actual_log))?;
    operator_log::info(
        module_path!(),
        format!(
            "service {} ready pid={:?} log={}",
            service_id,
            pid,
            actual_log.display()
        ),
    );
    let mut summary = ServiceSummary::new(service_id, pid);
    summary.add_detail(format!("log={}", actual_log.display()));
    Ok(summary)
}

fn print_service_summary(summaries: &[ServiceSummary]) {
    if summaries.is_empty() {
        return;
    }
    println!("\nStarted services:");
    for summary in summaries {
        println!("{}", summary.describe());
    }
}

fn spawn_embedded_messaging(
    bundle_root: &Path,
    tenant: &str,
    team: &str,
    env: BTreeMap<String, String>,
    log_dir: &Path,
    restart: &BTreeSet<String>,
    tracker: &mut ServiceTracker,
) -> anyhow::Result<ServiceSummary> {
    let exe = env::current_exe()?;
    let mut args = vec![
        "dev".to_string(),
        "embedded".to_string(),
        "--project-root".to_string(),
        bundle_root.display().to_string(),
        "--no-nats".to_string(),
    ];
    let mut argv = vec![exe.to_string_lossy().to_string()];
    argv.append(&mut args);

    let spec = supervisor::ServiceSpec {
        id: supervisor::ServiceId::new("messaging")?,
        argv,
        cwd: None,
        env,
    };

    let mut summary = spawn_supervised_service(
        "messaging",
        "messaging",
        &spec,
        log_dir,
        tracker.paths,
        restart,
        tracker,
    )?;
    summary.add_detail(format!("tenant={tenant} team={team}"));
    summary.add_detail(format!(
        "cmd=dev embedded --project-root {}",
        bundle_root.display()
    ));
    Ok(summary)
}

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
    log_dir: &Path,
) -> anyhow::Result<()> {
    let team_id = team.unwrap_or("default");
    let state_dir = bundle_root.join("state");
    let paths = RuntimePaths::new(&state_dir, tenant, team_id);
    let mut service_tracker = ServiceTracker::new(&paths, Some(log_dir))?;
    let mut service_summaries = Vec::new();
    let restart_targets = BTreeSet::new();
    let mut public_base_url: Option<String> = None;
    if let Some(config) = cloudflared {
        let cloudflared_log = operator_log::reserve_service_log(log_dir, "cloudflared")
            .with_context(|| "unable to open cloudflared.log")?;
        operator_log::info(
            module_path!(),
            format!(
                "starting cloudflared log={} bundle={}",
                cloudflared_log.display(),
                bundle_root.display()
            ),
        );
        let handle = cloudflared::start_quick_tunnel(&paths, &config, &cloudflared_log)?;
        operator_log::info(
            module_path!(),
            format!(
                "cloudflared ready url={} log={}",
                handle.url,
                handle.log_path.display()
            ),
        );
        let url = handle.url.clone();
        let log_path = handle.log_path.clone();
        service_tracker.record_with_log("cloudflared", "cloudflared", Some(&log_path))?;
        let summary = ServiceSummary::with_details(
            "cloudflared",
            Some(handle.pid),
            vec![
                format!("url={}", url),
                format!("log={}", log_path.display()),
            ],
        );
        service_summaries.push(summary);
        public_base_url = Some(url.clone());
        println!("Public URL (service=cloudflared): {}", url);
    }

    let mut resolved_nats_url = nats_url.map(|value| value.to_string());
    if !no_nats && resolved_nats_url.is_none() {
        match operator_log::reserve_service_log(log_dir, "nats") {
            Ok(nats_log) => {
                operator_log::info(
                    module_path!(),
                    format!("starting nats log={}", nats_log.display()),
                );
                match services::start_nats_with_log(bundle_root, Some(nats_log.clone())) {
                    Ok(state) => {
                        operator_log::info(
                            module_path!(),
                            format!("nats started state={:?} log={}", state, nats_log.display()),
                        );
                        service_tracker
                            .record_with_log("nats", "nats", Some(&nats_log))
                            .with_context(|| "failed to record nats service state")?;
                        resolved_nats_url = Some(services::nats_url(bundle_root));
                        let pid = read_pid(&paths.pid_path("nats"))?;
                        let mut summary = ServiceSummary::new("nats", pid);
                        summary.add_detail(format!("state={:?}", state));
                        summary.add_detail(format!("url={}", services::nats_url(bundle_root)));
                        summary.add_detail(format!("log={}", nats_log.display()));
                        service_summaries.push(summary);
                        mark_nats_started(&paths)?;
                    }
                    Err(err) => {
                        eprintln!("Warning: failed to start NATS: {err}");
                        operator_log::error(
                            module_path!(),
                            format!("failed to start nats (log={}): {err}", nats_log.display()),
                        );
                    }
                }
            }
            Err(err) => {
                eprintln!("Warning: failed to prepare NATS log: {err}");
                operator_log::error(module_path!(), format!("failed to open nats.log: {err}"));
            }
        }
    }

    if messaging_enabled {
        let env_map = build_env(
            tenant,
            team_id,
            resolved_nats_url.as_deref(),
            public_base_url.as_deref(),
        );
        let mut messaging_summary = spawn_embedded_messaging(
            bundle_root,
            tenant,
            team_id,
            env_map,
            log_dir,
            &restart_targets,
            &mut service_tracker,
        )?;
        messaging_summary.add_detail("embedded messaging stack".to_string());
        service_summaries.push(messaging_summary);
    } else {
        println!("messaging: skipped (disabled or no providers)");
    }

    if !events_components.is_empty() && resolved_nats_url.is_some() {
        let envs = build_env_kv(tenant, team, resolved_nats_url.as_deref());
        for component in events_components {
            let state = services::start_component(bundle_root, &component, &envs)?;
            println!("{}: {:?}", component.id, state);
            let log_path = paths.log_path(&component.id);
            service_tracker
                .record_with_log(component.id.clone(), "component", Some(log_path.as_path()))
                .with_context(|| format!("failed to record component {}", component.id))?;
            let pid = read_pid(&paths.pid_path(&component.id))?;
            let mut summary = ServiceSummary::new(component.id.clone(), pid);
            summary.add_detail(format!("state={:?}", state));
            summary.add_detail(format!("log={}", log_path.display()));
            service_summaries.push(summary);
        }
    } else {
        println!("events: skipped (disabled or no providers)");
    }
    print_service_summary(&service_summaries);

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
    log_dir: &Path,
) -> anyhow::Result<()> {
    let config_dir = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config path has no parent directory"))?;
    let state_dir = config_dir.join("state");
    let tenant = config.tenant.as_str();
    let team = config.team.as_str();
    let paths = RuntimePaths::new(&state_dir, tenant, team);
    let mut service_tracker = ServiceTracker::new(&paths, Some(log_dir))?;
    let discovery = crate::discovery::discover(config_dir)?;
    crate::discovery::persist(config_dir, tenant, &discovery)?;
    operator_log::info(
        module_path!(),
        format!(
            "demo up services start bundle={} tenant={} team={} log_dir={}",
            config_path.display(),
            tenant,
            team,
            log_dir.display()
        ),
    );

    if should_restart(restart, "cloudflared") {
        let _ = supervisor::stop_pidfile(&paths.pid_path("cloudflared"), 2_000);
    }

    let public_base_url = if let Some(cfg) = cloudflared {
        let cloudflared_log = operator_log::reserve_service_log(log_dir, "cloudflared")
            .with_context(|| "unable to open cloudflared.log")?;
        operator_log::info(
            module_path!(),
            format!("starting cloudflared log={}", cloudflared_log.display()),
        );
        let handle = cloudflared::start_quick_tunnel(&paths, &cfg, &cloudflared_log)?;
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
        operator_log::info(
            module_path!(),
            format!(
                "cloudflared ready domains={} url={} log={}",
                domain_list,
                handle.url,
                handle.log_path.display()
            ),
        );
        println!(
            "Public URL (service=cloudflared domains={domain_list}): {}",
            handle.url
        );
        service_tracker.record_with_log("cloudflared", "cloudflared", Some(&handle.log_path))?;
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
            let nats_log = operator_log::reserve_service_log(log_dir, "nats")
                .with_context(|| "unable to open nats.log")?;
            if let Some(handle) = spawn_if_needed(&paths, &spec, restart, Some(nats_log.clone()))? {
                service_tracker
                    .record_with_log("nats", "nats", Some(&handle.log_path))
                    .with_context(|| "failed to record nats service")?;
            }
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
            if let Some(handle) = spawn_if_needed(&paths, &spec, restart, None)? {
                service_tracker
                    .record_with_log(component.id.clone(), "component", Some(&handle.log_path))
                    .with_context(|| format!("failed to record {}", component.id))?;
            }
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
    if let Some(handle) = spawn_if_needed(&paths, &gateway_spec, restart, None)? {
        service_tracker.record_with_log("gateway", "gateway", Some(&handle.log_path))?;
    }

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
    if let Some(handle) = spawn_if_needed(&paths, &egress_spec, restart, None)? {
        service_tracker.record_with_log("egress", "egress", Some(&handle.log_path))?;
    }

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
        if let Some(handle) = spawn_if_needed(&paths, &spec, restart, None)? {
            service_tracker.record_with_log(
                "subscriptions",
                "subscriptions",
                Some(&handle.log_path),
            )?;
        }
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
    log_dir: &Path,
    tenant: &str,
    team: &str,
    service: &str,
    tail: bool,
) -> anyhow::Result<()> {
    let log_dir = resolve_manifest_log_dir(state_dir, tenant, team, log_dir)?;
    let log_path = if service == "operator" {
        log_dir.join("operator.log")
    } else {
        let tenant_log_path = tenant_log_path(&log_dir, service, tenant, team)?;
        select_log_path(&log_dir, service, tenant, &tenant_log_path)
    };
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
    let paths = RuntimePaths::new(state_dir, tenant, team);
    stop_started_nats(&paths, state_dir)?;
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
        remove_service_manifest(&paths)?;
        println!("Stopped all services under {}", pids_root.display());
        return Ok(());
    }

    if let Some(manifest) = read_service_manifest(&paths)? {
        if manifest.services.is_empty() {
            println!("No services to stop.");
            return Ok(());
        }
        for entry in manifest.services.iter().rev() {
            let id = supervisor::ServiceId::new(entry.id.clone())?;
            if let Err(err) = supervisor::stop_service(&paths, &id, timeout_ms) {
                eprintln!("Warning: failed to stop {}: {err}", entry.id);
            }
        }
        remove_service_manifest(&paths)?;
        return Ok(());
    }

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

fn select_log_path(log_dir: &Path, service: &str, tenant: &str, tenant_log: &Path) -> PathBuf {
    let mut candidates = Vec::new();
    candidates.push(tenant_log.to_path_buf());
    candidates.push(log_dir.join(format!("{service}.log")));
    candidates.push(log_dir.join(format!("{service}-{tenant}.log")));
    candidates.push(log_dir.join(format!("{service}.{tenant}.log")));
    for candidate in &candidates {
        if candidate.exists() {
            return candidate.clone();
        }
    }
    tenant_log.to_path_buf()
}

fn tenant_log_path(
    log_dir: &Path,
    service: &str,
    tenant: &str,
    team: &str,
) -> anyhow::Result<PathBuf> {
    let tenant_dir = log_dir.join(format!("{tenant}.{team}"));
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

fn resolve_manifest_log_dir(
    state_dir: &Path,
    tenant: &str,
    team: &str,
    default: &Path,
) -> anyhow::Result<PathBuf> {
    let paths = RuntimePaths::new(state_dir, tenant, team);
    if let Some(manifest) = read_service_manifest(&paths)?
        && let Some(dir) = manifest.log_dir
    {
        return Ok(PathBuf::from(dir));
    }
    Ok(default.to_path_buf())
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

fn mark_nats_started(paths: &RuntimePaths) -> anyhow::Result<()> {
    let marker = nats_started_marker(paths);
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(marker, "started")?;
    Ok(())
}

fn stop_started_nats(paths: &RuntimePaths, state_dir: &Path) -> anyhow::Result<()> {
    let marker = nats_started_marker(paths);
    if !marker.exists() {
        return Ok(());
    }
    let bundle_root = state_dir.parent().unwrap_or(state_dir);
    match services::stop_nats(bundle_root) {
        Ok(_) => {
            let _ = std::fs::remove_file(&marker);
        }
        Err(err) => {
            eprintln!("Warning: failed to stop nats: {err}");
        }
    }
    Ok(())
}

fn nats_started_marker(paths: &RuntimePaths) -> PathBuf {
    paths.runtime_root().join("nats.started")
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
    log_path_override: Option<PathBuf>,
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
    let handle = supervisor::spawn_service(paths, spec.clone(), log_path_override.clone())?;
    println!("{}: started (pid={})", spec.id.as_str(), handle.pid);
    if spec.id.as_str() == "nats" {
        operator_log::info(
            module_path!(),
            format!(
                "spawned nats pid={} log={}",
                handle.pid,
                handle.log_path.display()
            ),
        );
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn tenant_log_path_creates_file() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let path = tenant_log_path(dir.path(), "messaging", "demo", "default")?;
        assert!(path.exists());
        Ok(())
    }

    #[test]
    fn select_log_path_prefers_existing() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let tenant_path = tenant_log_path(dir.path(), "messaging", "demo", "default")?;
        fs::write(dir.path().join("messaging.log"), "other")?;
        let selected = select_log_path(dir.path(), "messaging", "demo", &tenant_path);
        assert_eq!(selected, tenant_path);
        Ok(())
    }

    #[test]
    fn demo_logs_runtime_reads_operator_log() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let log = dir.path().join("operator.log");
        fs::write(&log, "operator ready")?;
        demo_logs_runtime(dir.path(), dir.path(), "demo", "default", "operator", false)?;
        Ok(())
    }
}
