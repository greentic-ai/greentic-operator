use std::collections::HashMap;
use std::fs;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, anyhow};
use base64::{Engine as _, engine::general_purpose};
use greentic_runner_desktop::RunStatus;
use greentic_runner_host::{
    RunnerWasiPolicy,
    component_api::node::{ExecCtx as ComponentExecCtx, TenantCtx as ComponentTenantCtx},
    config::{
        FlowRetryConfig, HostConfig, OperatorPolicy, RateLimits, SecretsPolicy, StateStorePolicy,
        WebhookPolicy,
    },
    pack::{ComponentResolution, PackRuntime},
    secrets::default_manager,
    storage::{DynSessionStore, new_state_store},
    trace::TraceConfig,
    validate::ValidationConfig,
};
use greentic_types::decode_pack_manifest;
use serde_json::{Value as JsonValue, json};
use tokio::runtime::Runtime as TokioRuntime;
use zip::ZipArchive;

use crate::runner_exec;
use crate::runner_integration;
use crate::runner_integration::RunFlowOptions;
use crate::runner_integration::RunnerFlavor;
use crate::runner_integration::run_flow_with_options;

use crate::cards::CardRenderer;
use crate::discovery;
use crate::domains::{self, Domain, ProviderPack};
use crate::operator_log;
use crate::secrets_gate::DynSecretsManager;
use crate::state_layout;
use messaging_cardkit::Tier;

#[derive(Clone)]
pub struct OperatorContext {
    pub tenant: String,
    pub team: Option<String>,
    pub correlation_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerExecutionMode {
    Exec,
    Integration,
}

#[derive(Clone)]
pub struct FlowOutcome {
    pub success: bool,
    pub output: Option<JsonValue>,
    pub raw: Option<String>,
    pub error: Option<String>,
    pub mode: RunnerExecutionMode,
}

#[derive(Clone, Debug)]
enum RunnerMode {
    Exec,
    Integration {
        binary: PathBuf,
        flavor: RunnerFlavor,
    },
}

#[derive(Clone)]
pub struct DemoRunnerHost {
    bundle_root: PathBuf,
    runner_mode: RunnerMode,
    catalog: HashMap<(Domain, String), ProviderPack>,
    _secrets_manager: DynSecretsManager,
    card_renderer: CardRenderer,
    debug_enabled: bool,
}

impl DemoRunnerHost {
    pub fn bundle_root(&self) -> &Path {
        &self.bundle_root
    }

    pub fn secrets_manager(&self) -> DynSecretsManager {
        self._secrets_manager.clone()
    }

    pub fn new(
        bundle_root: PathBuf,
        discovery: &discovery::DiscoveryResult,
        runner_binary: Option<PathBuf>,
        secrets_manager: DynSecretsManager,
        debug_enabled: bool,
    ) -> anyhow::Result<Self> {
        let runner_binary = runner_binary.and_then(validate_runner_binary);
        let mode = if let Some(ref binary) = runner_binary {
            let flavor = runner_integration::detect_runner_flavor(binary);
            RunnerMode::Integration {
                binary: binary.clone(),
                flavor,
            }
        } else {
            RunnerMode::Exec
        };
        let mut catalog = HashMap::new();
        let provider_map = discovery
            .providers
            .iter()
            .map(|provider| (provider.pack_path.clone(), provider.provider_id.clone()))
            .collect::<HashMap<_, _>>();
        for domain in [Domain::Messaging, Domain::Events, Domain::Secrets] {
            let is_demo_bundle = bundle_root.join("greentic.demo.yaml").exists();
            let packs = if is_demo_bundle {
                domains::discover_provider_packs_cbor_only(&bundle_root, domain)?
            } else {
                domains::discover_provider_packs(&bundle_root, domain)?
            };
            for pack in packs {
                let provider_type = provider_map
                    .get(&pack.path)
                    .cloned()
                    .unwrap_or_else(|| pack.pack_id.clone());
                catalog.insert((domain, provider_type.clone()), pack.clone());
                if provider_type != pack.pack_id {
                    catalog.insert((domain, pack.pack_id.clone()), pack.clone());
                }
            }
        }
        Ok(Self {
            bundle_root,
            runner_mode: mode,
            catalog,
            _secrets_manager: secrets_manager,
            card_renderer: CardRenderer::new(Tier::Premium),
            debug_enabled,
        })
    }

    pub fn debug_enabled(&self) -> bool {
        self.debug_enabled
    }

    pub fn supports_op(&self, domain: Domain, provider_type: &str, op_id: &str) -> bool {
        self.catalog
            .get(&(domain, provider_type.to_string()))
            .map(|pack| pack.entry_flows.iter().any(|flow| flow == op_id))
            .unwrap_or(false)
    }

    pub fn invoke_provider_op(
        &self,
        domain: Domain,
        provider_type: &str,
        op_id: &str,
        payload_bytes: &[u8],
        ctx: &OperatorContext,
    ) -> anyhow::Result<FlowOutcome> {
        let pack = self
            .catalog
            .get(&(domain, provider_type.to_string()))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "provider {} not found for domain {}",
                    provider_type,
                    domains::domain_name(domain)
                )
            })?;

        if pack.entry_flows.iter().any(|flow| flow == op_id) {
            let flow_id = op_id;
            if self.debug_enabled {
                operator_log::debug(
                    module_path!(),
                    format!(
                        "[demo dev] invoking provider domain={} provider={} flow={} tenant={} team={} payload_len={} preview={}",
                        domains::domain_name(domain),
                        provider_type,
                        flow_id,
                        ctx.tenant,
                        ctx.team.as_deref().unwrap_or("default"),
                        payload_bytes.len(),
                        payload_preview(payload_bytes),
                    ),
                );
            }
            let run_dir = state_layout::run_dir(&self.bundle_root, domain, &pack.pack_id, flow_id)?;
            std::fs::create_dir_all(&run_dir)?;

            let render_outcome = self
                .card_renderer
                .render_if_needed(provider_type, payload_bytes)?;
            if let Some(meta) = &render_outcome.metadata {
                operator_log::info(
                    module_path!(),
                    format!(
                        "render provider={} tier={} target={} downgraded={} warnings={} corr={}",
                        provider_type,
                        meta.tier.as_str(),
                        meta.target_tier.as_str(),
                        meta.downgraded,
                        meta.warnings_count,
                        ctx.correlation_id.as_deref().unwrap_or("none"),
                    ),
                );
            }
            let payload = serde_json::from_slice(&render_outcome.bytes).unwrap_or_else(|_| {
                json!({
                    "payload": general_purpose::STANDARD.encode(&render_outcome.bytes)
                })
            });

            let outcome = match &self.runner_mode {
                RunnerMode::Exec => {
                    self.execute_with_runner_exec(domain, pack, flow_id, &payload, ctx, &run_dir)?
                }
                RunnerMode::Integration { binary, flavor } => self
                    .execute_with_runner_integration(
                        domain, pack, flow_id, &payload, ctx, &run_dir, binary, *flavor,
                    )?,
            };

            if self.debug_enabled {
                operator_log::debug(
                    module_path!(),
                    format!(
                        "[demo dev] provider={} flow={} tenant={} team={} success={} mode={:?} error={:?} corr_id={}",
                        provider_type,
                        flow_id,
                        ctx.tenant,
                        ctx.team.as_deref().unwrap_or("default"),
                        outcome.success,
                        outcome.mode,
                        outcome.error,
                        ctx.correlation_id.as_deref().unwrap_or("none"),
                    ),
                );
            }
            operator_log::info(
                module_path!(),
                format!(
                    "invoke domain={} provider={} op={} mode={:?} corr={}",
                    domains::domain_name(domain),
                    provider_type,
                    flow_id,
                    outcome.mode,
                    ctx.correlation_id.as_deref().unwrap_or("none")
                ),
            );

            return Ok(outcome);
        }

        self.invoke_provider_component_op(domain, pack, provider_type, op_id, payload_bytes, ctx)
    }

    fn execute_with_runner_exec(
        &self,
        domain: Domain,
        pack: &ProviderPack,
        flow_id: &str,
        payload: &JsonValue,
        ctx: &OperatorContext,
        _run_dir: &Path,
    ) -> anyhow::Result<FlowOutcome> {
        let request = runner_exec::RunRequest {
            root: self.bundle_root.clone(),
            domain,
            pack_path: pack.path.clone(),
            pack_label: pack.pack_id.clone(),
            flow_id: flow_id.to_string(),
            tenant: ctx.tenant.clone(),
            team: ctx.team.clone(),
            input: payload.clone(),
            dist_offline: true,
        };
        let run_output = runner_exec::run_provider_pack_flow(request)?;
        let parsed = read_transcript_outputs(&run_output.run_dir)?;
        Ok(FlowOutcome {
            success: run_output.result.status == RunStatus::Success,
            output: parsed,
            raw: None,
            error: run_output.result.error.clone(),
            mode: RunnerExecutionMode::Exec,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_with_runner_integration(
        &self,
        _domain: Domain,
        pack: &ProviderPack,
        flow_id: &str,
        payload: &JsonValue,
        ctx: &OperatorContext,
        run_dir: &Path,
        runner_binary: &Path,
        flavor: RunnerFlavor,
    ) -> anyhow::Result<FlowOutcome> {
        let output = run_flow_with_options(
            runner_binary,
            &pack.path,
            flow_id,
            payload,
            RunFlowOptions {
                dist_offline: true,
                tenant: Some(&ctx.tenant),
                team: ctx.team.as_deref(),
                artifacts_dir: Some(run_dir),
                runner_flavor: flavor,
            },
        )?;
        let mut parsed = output.parsed.clone();
        if parsed.is_none() {
            parsed = read_transcript_outputs(run_dir)?;
        }
        let raw = if output.stdout.trim().is_empty() {
            None
        } else {
            Some(output.stdout.clone())
        };
        Ok(FlowOutcome {
            success: output.status.success(),
            output: parsed,
            raw,
            error: if output.status.success() {
                None
            } else {
                Some(output.stderr.clone())
            },
            mode: RunnerExecutionMode::Integration,
        })
    }

    pub fn invoke_provider_component_op_direct(
        &self,
        domain: Domain,
        pack: &ProviderPack,
        provider_id: &str,
        op_id: &str,
        payload_bytes: &[u8],
        ctx: &OperatorContext,
    ) -> anyhow::Result<FlowOutcome> {
        self.invoke_provider_component_op(domain, pack, provider_id, op_id, payload_bytes, ctx)
    }

    fn invoke_provider_component_op(
        &self,
        _domain: Domain,
        pack: &ProviderPack,
        _provider_id: &str,
        op_id: &str,
        payload_bytes: &[u8],
        ctx: &OperatorContext,
    ) -> anyhow::Result<FlowOutcome> {
        let secrets_manager = default_manager()
            .context("failed to create secrets manager for provider invocation")?;
        let runtime = TokioRuntime::new()
            .context("failed to create tokio runtime for provider invocation")?;
        let payload = payload_bytes.to_vec();
        let result = runtime.block_on(async {
            let host_config = Arc::new(build_demo_host_config(&ctx.tenant));
            let pack_runtime = PackRuntime::load(
                &pack.path,
                host_config.clone(),
                None,
                Some(&pack.path),
                None::<DynSessionStore>,
                Some(new_state_store()),
                Arc::new(RunnerWasiPolicy::default()),
                secrets_manager.clone(),
                None,
                false,
                ComponentResolution::default(),
            )
            .await?;
            let provider_type = primary_provider_type(&pack.path)
                .context("failed to determine provider type for direct invocation")?;
            let binding = pack_runtime.resolve_provider(None, Some(&provider_type))?;
            let exec_ctx = ComponentExecCtx {
                tenant: ComponentTenantCtx {
                    tenant: ctx.tenant.clone(),
                    team: ctx.team.clone(),
                    user: None,
                    trace_id: None,
                    correlation_id: ctx.correlation_id.clone(),
                    deadline_unix_ms: None,
                    attempt: 1,
                    idempotency_key: None,
                },
                flow_id: op_id.to_string(),
                node_id: Some(op_id.to_string()),
            };
            pack_runtime
                .invoke_provider(&binding, exec_ctx, op_id, payload)
                .await
        });

        match result {
            Ok(value) => Ok(FlowOutcome {
                success: true,
                output: Some(value),
                raw: None,
                error: None,
                mode: RunnerExecutionMode::Exec,
            }),
            Err(err) => Ok(FlowOutcome {
                success: false,
                output: None,
                raw: None,
                error: Some(err.to_string()),
                mode: RunnerExecutionMode::Exec,
            }),
        }
    }
}

pub fn primary_provider_type(pack_path: &Path) -> anyhow::Result<String> {
    let file = std::fs::File::open(pack_path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut manifest_entry = archive.by_name("manifest.cbor").map_err(|err| {
        anyhow!(
            "failed to open manifest.cbor in {}: {err}",
            pack_path.display()
        )
    })?;
    let mut bytes = Vec::new();
    manifest_entry.read_to_end(&mut bytes)?;
    let manifest = decode_pack_manifest(&bytes)
        .context("failed to decode pack manifest for provider introspection")?;
    let inline = manifest.provider_extension_inline().ok_or_else(|| {
        anyhow!(
            "pack {} provider extension missing or not inline",
            pack_path.display()
        )
    })?;
    let provider = inline.providers.first().ok_or_else(|| {
        anyhow!(
            "pack {} provider extension contains no providers",
            pack_path.display()
        )
    })?;
    Ok(provider.provider_type.clone())
}

fn read_transcript_outputs(run_dir: &Path) -> anyhow::Result<Option<JsonValue>> {
    let path = run_dir.join("transcript.jsonl");
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)?;
    let mut last = None;
    for line in contents.lines() {
        let Ok(value) = serde_json::from_str::<JsonValue>(line) else {
            continue;
        };
        let Some(outputs) = value.get("outputs") else {
            continue;
        };
        if !outputs.is_null() {
            last = Some(outputs.clone());
        }
    }
    Ok(last)
}

fn build_demo_host_config(tenant: &str) -> HostConfig {
    HostConfig {
        tenant: tenant.to_string(),
        bindings_path: PathBuf::from("<demo-provider>"),
        flow_type_bindings: HashMap::new(),
        rate_limits: RateLimits::default(),
        retry: FlowRetryConfig::default(),
        http_enabled: true,
        secrets_policy: SecretsPolicy::allow_all(),
        state_store_policy: StateStorePolicy::default(),
        webhook_policy: WebhookPolicy::default(),
        timers: Vec::new(),
        oauth: None,
        mocks: None,
        pack_bindings: Vec::new(),
        env_passthrough: Vec::new(),
        trace: TraceConfig::from_env(),
        validation: ValidationConfig::from_env(),
        operator_policy: OperatorPolicy::allow_all(),
    }
}

fn validate_runner_binary(path: PathBuf) -> Option<PathBuf> {
    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() && runner_binary_is_executable(&metadata) => Some(path),
        Ok(metadata) => {
            let reason = if !metadata.is_file() {
                "not a regular file"
            } else {
                "not executable"
            };
            operator_log::warn(
                module_path!(),
                format!(
                    "runner binary '{}' is not usable ({})",
                    path.display(),
                    reason
                ),
            );
            None
        }
        Err(err) => {
            operator_log::warn(
                module_path!(),
                format!(
                    "runner binary '{}' cannot be accessed: {}",
                    path.display(),
                    err
                ),
            );
            None
        }
    }
}

#[cfg(unix)]
fn runner_binary_is_executable(metadata: &fs::Metadata) -> bool {
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn runner_binary_is_executable(_: &fs::Metadata) -> bool {
    true
}

fn payload_preview(bytes: &[u8]) -> String {
    const MAX_PREVIEW: usize = 256;
    if bytes.is_empty() {
        return "<empty>".to_string();
    }
    let preview_len = bytes.len().min(MAX_PREVIEW);
    if let Ok(text) = std::str::from_utf8(&bytes[..preview_len]) {
        if bytes.len() <= MAX_PREVIEW {
            text.to_string()
        } else {
            format!("{text}...")
        }
    } else {
        let encoded = general_purpose::STANDARD.encode(&bytes[..preview_len]);
        if bytes.len() <= MAX_PREVIEW {
            encoded
        } else {
            format!("{encoded}...")
        }
    }
}
