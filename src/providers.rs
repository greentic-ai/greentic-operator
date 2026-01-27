use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::bin_resolver::{self, ResolveCtx};
use crate::config::{DemoConfig, DemoProviderConfig};
use crate::dev_mode::DevSettingsResolved;
use crate::operator_log;
use crate::runner_integration;
use crate::runtime_state::RuntimePaths;
use crate::setup_input::{SetupInputAnswers, collect_setup_answers, load_setup_input};
use greentic_runner_desktop::{RunResult, RunStatus};

pub struct ProviderSetupOptions {
    pub providers: Option<Vec<String>>,
    pub verify_webhooks: bool,
    pub force_setup: bool,
    pub skip_setup: bool,
    pub skip_secrets_init: bool,
    pub setup_input: Option<PathBuf>,
    pub runner_binary: Option<PathBuf>,
    pub continue_on_error: bool,
}

pub fn run_provider_setup(
    config_dir: &Path,
    config: &DemoConfig,
    dev_settings: Option<DevSettingsResolved>,
    public_base_url: Option<&str>,
    options: ProviderSetupOptions,
) -> anyhow::Result<()> {
    let providers = resolve_providers(config, options.providers);
    if providers.is_empty() || options.skip_setup {
        return Ok(());
    }

    let runner = resolve_runner_binary(config_dir, dev_settings.as_ref(), options.runner_binary)?;
    let secrets_bin = if options.skip_secrets_init {
        None
    } else {
        Some(resolve_secrets_binary(config_dir, dev_settings.as_ref())?)
    };

    let runtime = RuntimePaths::new(
        config_dir.join("state"),
        config.tenant.clone(),
        config.team.clone(),
    );
    let providers_root = runtime.runtime_root().join("providers");
    std::fs::create_dir_all(&providers_root)?;
    let provider_keys: BTreeSet<String> = providers.iter().map(|(name, _)| name.clone()).collect();
    let setup_input_answers = if let Some(path) = options.setup_input.as_ref() {
        Some(SetupInputAnswers::new(
            load_setup_input(path)?,
            provider_keys.clone(),
        )?)
    } else {
        None
    };

    for (provider, cfg) in providers {
        let result = (|| -> anyhow::Result<()> {
            let pack_path = resolve_pack_path(config_dir, &provider, &cfg)?;
            if !options.skip_secrets_init {
                let env = crate::tools::secrets::resolve_env();
                let status = crate::tools::secrets::run_init(
                    config_dir,
                    secrets_bin.as_deref(),
                    &env,
                    &config.tenant,
                    Some(&config.team),
                    &pack_path,
                    true,
                )?;
                if !status.success() {
                    let code = status.code().unwrap_or(1);
                    return Err(anyhow::anyhow!(
                        "greentic-secrets init failed with exit code {code}"
                    ));
                }
            }

            let setup_path = providers_root.join(format!("{provider}.setup.json"));
            if setup_path.exists() && !options.force_setup {
                return Ok(());
            }

            let setup_flow = cfg
                .setup_flow
                .clone()
                .unwrap_or_else(|| "setup_default".to_string());
            let answers = collect_setup_answers(
                &pack_path,
                &provider,
                setup_input_answers.as_ref(),
                setup_input_answers.is_none(),
            )?;
            let input = build_input(
                &provider,
                &config.tenant,
                &config.team,
                public_base_url,
                Some(&answers),
            )?;
            let output = runner_integration::run_flow(&runner, &pack_path, &setup_flow, &input)?;
            write_run_output(&setup_path, &provider, &setup_flow, &output)?;

            if options.verify_webhooks {
                let verify_flow = cfg
                    .verify_flow
                    .clone()
                    .unwrap_or_else(|| "verify_webhooks".to_string());
                let verify_path = providers_root.join(format!("{provider}.verify.json"));
                if !verify_path.exists() || options.force_setup {
                    let output =
                        runner_integration::run_flow(&runner, &pack_path, &verify_flow, &input)?;
                    write_run_output(&verify_path, &provider, &verify_flow, &output)?;
                }
            }

            let status_path = providers_root.join(format!("{provider}.status.json"));
            write_status(&status_path, &provider, &setup_path)?;
            Ok(())
        })();
        if let Err(err) = result {
            if options.continue_on_error {
                operator_log::error(
                    module_path!(),
                    format!("provider setup failed for {provider}: {err}"),
                );
                continue;
            }
            return Err(err);
        }
    }

    Ok(())
}

fn resolve_providers(
    config: &DemoConfig,
    filter: Option<Vec<String>>,
) -> Vec<(String, DemoProviderConfig)> {
    let mut selected = Vec::new();
    let Some(map) = config.providers.as_ref() else {
        return selected;
    };
    let filter_set = filter.map(|list| {
        list.into_iter()
            .map(|value| value.trim().to_string())
            .collect::<std::collections::BTreeSet<_>>()
    });
    for (name, cfg) in map {
        if let Some(filter_set) = filter_set.as_ref()
            && !filter_set.contains(name)
        {
            continue;
        }
        selected.push((name.clone(), cfg.clone()));
    }
    selected
}

fn resolve_runner_binary(
    config_dir: &Path,
    dev_settings: Option<&DevSettingsResolved>,
    explicit: Option<PathBuf>,
) -> anyhow::Result<PathBuf> {
    let explicit = explicit.map(|path| {
        if path.is_absolute() {
            path
        } else {
            config_dir.join(path)
        }
    });
    bin_resolver::resolve_binary(
        "greentic-runner",
        &ResolveCtx {
            config_dir: config_dir.to_path_buf(),
            dev: dev_settings.cloned(),
            explicit_path: explicit,
        },
    )
}

fn resolve_secrets_binary(
    config_dir: &Path,
    dev_settings: Option<&DevSettingsResolved>,
) -> anyhow::Result<PathBuf> {
    bin_resolver::resolve_binary(
        "greentic-secrets",
        &ResolveCtx {
            config_dir: config_dir.to_path_buf(),
            dev: dev_settings.cloned(),
            explicit_path: None,
        },
    )
}

fn resolve_pack_path(
    config_dir: &Path,
    provider: &str,
    cfg: &DemoProviderConfig,
) -> anyhow::Result<PathBuf> {
    if let Some(pack) = cfg.pack.as_ref() {
        let path = Path::new(pack);
        return Ok(if path.is_absolute() {
            path.to_path_buf()
        } else {
            config_dir.join(path)
        });
    }
    let default_dir = if config_dir.join("provider-packs").exists() {
        config_dir.join("provider-packs")
    } else {
        config_dir.join("demo").join("provider-packs")
    };
    Ok(default_dir.join(format!("{provider}.gtpack")))
}

fn build_input(
    pack_id: &str,
    tenant: &str,
    team: &str,
    public_base_url: Option<&str>,
    answers: Option<&Value>,
) -> anyhow::Result<Value> {
    let mut payload = serde_json::json!({
        "id": pack_id,
        "tenant": tenant,
        "team": team,
        "env": "dev",
    });
    let mut config = serde_json::json!({});
    if let Some(url) = public_base_url {
        payload["public_base_url"] = Value::String(url.to_string());
        config["public_base_url"] = Value::String(url.to_string());
    }
    config["id"] = Value::String(pack_id.to_string());
    payload["config"] = config;
    payload["msg"] = serde_json::json!({
        "channel": "setup",
        "id": format!("{pack_id}.setup"),
        "message": {
            "id": format!("{pack_id}.setup_default__collect"),
            "text": "Collect inputs for setup_default."
        },
        "metadata": serde_json::json!({}),
        "reply_scope": "",
        "session_id": "setup",
        "tenant_id": tenant,
        "text": "Collect inputs for setup_default.",
        "user_id": "operator"
    });
    payload["payload"] = serde_json::json!({});
    if let Some(answers) = answers {
        payload["setup_answers"] = answers.clone();
        payload["answers_json"] = Value::String(serde_json::to_string(answers)?);
    }
    Ok(payload)
}

pub(crate) fn write_run_output(
    path: &Path,
    provider: &str,
    flow: &str,
    output: &runner_integration::RunnerOutput,
) -> anyhow::Result<()> {
    let record = ProviderRunRecord {
        provider: provider.to_string(),
        flow: flow.to_string(),
        status: output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "terminated".to_string()),
        success: output.status.success(),
        stdout: output.stdout.clone(),
        stderr: output.stderr.clone(),
        parsed: output.parsed.clone(),
        timestamp: Utc::now(),
    };
    let bytes = serde_json::to_vec_pretty(&record)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

pub(crate) fn write_run_result(
    path: &Path,
    provider: &str,
    flow: &str,
    result: &RunResult,
) -> anyhow::Result<()> {
    let parsed = serde_json::to_value(result).ok();
    let record = ProviderRunRecord {
        provider: provider.to_string(),
        flow: flow.to_string(),
        status: format!("{:?}", result.status),
        success: result.status == RunStatus::Success,
        stdout: String::new(),
        stderr: result.error.clone().unwrap_or_default(),
        parsed,
        timestamp: Utc::now(),
    };
    let bytes = serde_json::to_vec_pretty(&record)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_status(path: &Path, provider: &str, setup_path: &Path) -> anyhow::Result<()> {
    let status = ProviderStatus {
        provider: provider.to_string(),
        setup_path: setup_path.to_path_buf(),
        updated_at: Utc::now(),
    };
    let bytes = serde_json::to_vec_pretty(&status)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[derive(Serialize)]
struct ProviderRunRecord {
    provider: String,
    flow: String,
    status: String,
    success: bool,
    stdout: String,
    stderr: String,
    parsed: Option<Value>,
    timestamp: DateTime<Utc>,
}

#[derive(Serialize)]
struct ProviderStatus {
    provider: String,
    setup_path: PathBuf,
    updated_at: DateTime<Utc>,
}
