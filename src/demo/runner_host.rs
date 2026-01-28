use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose};
use greentic_runner_desktop::RunStatus;
use serde_json::{Value as JsonValue, json};

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
            RunnerMode::Integration { binary, flavor } => self.execute_with_runner_integration(
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

        Ok(outcome)
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
