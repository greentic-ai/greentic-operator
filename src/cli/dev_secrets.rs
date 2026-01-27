use std::path::PathBuf;

use clap::{Parser, Subcommand};

use super::DevModeArgs;
use crate::bin_resolver::{self, ResolveCtx};
use crate::config;
use crate::tools::secrets;

#[derive(Parser)]
#[command(
    about = "Passthrough to greentic-secrets (init/set/get/list/delete).",
    long_about = "Forwards commands to greentic-secrets with tenant/team/env defaults and streams output to logs/secrets.",
    after_help = "Main options:\n  (none)\n\nOptional options:\n  (none)"
)]
pub struct DevSecretsCommand {
    #[command(subcommand)]
    command: DevSecretsSubcommand,
}

#[derive(Subcommand)]
enum DevSecretsSubcommand {
    Init(DevSecretsInitArgs),
    Set(DevSecretsSetArgs),
    Get(DevSecretsGetArgs),
    List(DevSecretsListArgs),
    Delete(DevSecretsDeleteArgs),
}

#[derive(Parser)]
#[command(
    about = "Initialize secrets for a provider pack.",
    long_about = "Runs greentic-secrets init with tenant/team/env context and non-interactive mode.",
    after_help = "Main options:\n  --tenant <TENANT>\n  --pack <PATH>\n\nOptional options:\n  --team <TEAM>\n  --env <ENV> (default: GREENTIC_ENV or dev)\n  --secrets-bin <PATH>\n  --project-root <PATH> (default: current directory)\n  --dev-mode <auto|on|off>\n  --dev-root <PATH>\n  --dev-profile <debug|release>\n  --dev-target-dir <PATH>"
)]
struct DevSecretsInitArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    env: Option<String>,
    #[arg(long)]
    pack: PathBuf,
    #[arg(long)]
    secrets_bin: Option<PathBuf>,
    #[arg(long)]
    project_root: Option<PathBuf>,
    #[command(flatten)]
    dev: DevModeArgs,
}

#[derive(Parser)]
#[command(
    about = "Set a secret via greentic-secrets.",
    long_about = "Runs greentic-secrets set with tenant/team/env context and forwards the key (and optional value).",
    after_help = "Main options:\n  <KEY>\n  --tenant <TENANT>\n\nOptional options:\n  <VALUE>\n  --team <TEAM>\n  --env <ENV> (default: GREENTIC_ENV or dev)\n  --secrets-bin <PATH>\n  --project-root <PATH> (default: current directory)\n  --dev-mode <auto|on|off>\n  --dev-root <PATH>\n  --dev-profile <debug|release>\n  --dev-target-dir <PATH>"
)]
struct DevSecretsSetArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    env: Option<String>,
    key: String,
    value: Option<String>,
    #[arg(long)]
    secrets_bin: Option<PathBuf>,
    #[arg(long)]
    project_root: Option<PathBuf>,
    #[command(flatten)]
    dev: DevModeArgs,
}

#[derive(Parser)]
#[command(
    about = "Get a secret via greentic-secrets.",
    long_about = "Runs greentic-secrets get with tenant/team/env context and forwards the key.",
    after_help = "Main options:\n  <KEY>\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --env <ENV> (default: GREENTIC_ENV or dev)\n  --secrets-bin <PATH>\n  --project-root <PATH> (default: current directory)\n  --dev-mode <auto|on|off>\n  --dev-root <PATH>\n  --dev-profile <debug|release>\n  --dev-target-dir <PATH>"
)]
struct DevSecretsGetArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    env: Option<String>,
    key: String,
    #[arg(long)]
    secrets_bin: Option<PathBuf>,
    #[arg(long)]
    project_root: Option<PathBuf>,
    #[command(flatten)]
    dev: DevModeArgs,
}

#[derive(Parser)]
#[command(
    about = "List secrets via greentic-secrets.",
    long_about = "Runs greentic-secrets list with tenant/team/env context.",
    after_help = "Main options:\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --env <ENV> (default: GREENTIC_ENV or dev)\n  --secrets-bin <PATH>\n  --project-root <PATH> (default: current directory)\n  --dev-mode <auto|on|off>\n  --dev-root <PATH>\n  --dev-profile <debug|release>\n  --dev-target-dir <PATH>"
)]
struct DevSecretsListArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    env: Option<String>,
    #[arg(long)]
    secrets_bin: Option<PathBuf>,
    #[arg(long)]
    project_root: Option<PathBuf>,
    #[command(flatten)]
    dev: DevModeArgs,
}

#[derive(Parser)]
#[command(
    about = "Delete a secret via greentic-secrets.",
    long_about = "Runs greentic-secrets delete with tenant/team/env context and forwards the key.",
    after_help = "Main options:\n  <KEY>\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --env <ENV> (default: GREENTIC_ENV or dev)\n  --secrets-bin <PATH>\n  --project-root <PATH> (default: current directory)\n  --dev-mode <auto|on|off>\n  --dev-root <PATH>\n  --dev-profile <debug|release>\n  --dev-target-dir <PATH>"
)]
struct DevSecretsDeleteArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    env: Option<String>,
    key: String,
    #[arg(long)]
    secrets_bin: Option<PathBuf>,
    #[arg(long)]
    project_root: Option<PathBuf>,
    #[command(flatten)]
    dev: DevModeArgs,
}

impl DevSecretsCommand {
    pub fn run(self, settings: &crate::settings::OperatorSettings) -> anyhow::Result<()> {
        match self.command {
            DevSecretsSubcommand::Init(args) => args.run(settings),
            DevSecretsSubcommand::Set(args) => args.run(settings),
            DevSecretsSubcommand::Get(args) => args.run(settings),
            DevSecretsSubcommand::List(args) => args.run(settings),
            DevSecretsSubcommand::Delete(args) => args.run(settings),
        }
    }
}

impl DevSecretsInitArgs {
    fn run(self, settings: &crate::settings::OperatorSettings) -> anyhow::Result<()> {
        let root = super::project_root(self.project_root)?;
        let env = self.env.unwrap_or_else(secrets::resolve_env);
        let config = config::load_operator_config(&root)?;
        let dev_settings =
            super::resolve_dev_settings(settings, config.as_ref(), &self.dev, &root)?;
        let explicit = self
            .secrets_bin
            .clone()
            .or_else(|| config::binary_override(config.as_ref(), "greentic-secrets", &root));
        let secrets_bin = bin_resolver::resolve_binary(
            "greentic-secrets",
            &ResolveCtx {
                config_dir: root.clone(),
                dev: dev_settings,
                explicit_path: explicit,
            },
        )?;
        let status = secrets::run_init(
            &root,
            Some(&secrets_bin),
            &env,
            &self.tenant,
            self.team.as_deref(),
            &self.pack,
            false,
        )?;
        if !status.success() {
            let code = status.code().unwrap_or(1);
            std::process::exit(code);
        }
        Ok(())
    }
}

impl DevSecretsSetArgs {
    fn run(self, settings: &crate::settings::OperatorSettings) -> anyhow::Result<()> {
        let root = super::project_root(self.project_root)?;
        let env = self.env.unwrap_or_else(secrets::resolve_env);
        let config = config::load_operator_config(&root)?;
        let dev_settings =
            super::resolve_dev_settings(settings, config.as_ref(), &self.dev, &root)?;
        let explicit = self
            .secrets_bin
            .clone()
            .or_else(|| config::binary_override(config.as_ref(), "greentic-secrets", &root));
        let secrets_bin = bin_resolver::resolve_binary(
            "greentic-secrets",
            &ResolveCtx {
                config_dir: root.clone(),
                dev: dev_settings,
                explicit_path: explicit,
            },
        )?;
        let status = secrets::run_set(
            &root,
            Some(&secrets_bin),
            &env,
            &self.tenant,
            self.team.as_deref(),
            &self.key,
            self.value.as_deref(),
        )?;
        if !status.success() {
            let code = status.code().unwrap_or(1);
            std::process::exit(code);
        }
        Ok(())
    }
}

impl DevSecretsGetArgs {
    fn run(self, settings: &crate::settings::OperatorSettings) -> anyhow::Result<()> {
        let root = super::project_root(self.project_root)?;
        let env = self.env.unwrap_or_else(secrets::resolve_env);
        let config = config::load_operator_config(&root)?;
        let dev_settings =
            super::resolve_dev_settings(settings, config.as_ref(), &self.dev, &root)?;
        let explicit = self
            .secrets_bin
            .clone()
            .or_else(|| config::binary_override(config.as_ref(), "greentic-secrets", &root));
        let secrets_bin = bin_resolver::resolve_binary(
            "greentic-secrets",
            &ResolveCtx {
                config_dir: root.clone(),
                dev: dev_settings,
                explicit_path: explicit,
            },
        )?;
        let status = secrets::run_get(
            &root,
            Some(&secrets_bin),
            &env,
            &self.tenant,
            self.team.as_deref(),
            &self.key,
        )?;
        if !status.success() {
            let code = status.code().unwrap_or(1);
            std::process::exit(code);
        }
        Ok(())
    }
}

impl DevSecretsListArgs {
    fn run(self, settings: &crate::settings::OperatorSettings) -> anyhow::Result<()> {
        let root = super::project_root(self.project_root)?;
        let env = self.env.unwrap_or_else(secrets::resolve_env);
        let config = config::load_operator_config(&root)?;
        let dev_settings =
            super::resolve_dev_settings(settings, config.as_ref(), &self.dev, &root)?;
        let explicit = self
            .secrets_bin
            .clone()
            .or_else(|| config::binary_override(config.as_ref(), "greentic-secrets", &root));
        let secrets_bin = bin_resolver::resolve_binary(
            "greentic-secrets",
            &ResolveCtx {
                config_dir: root.clone(),
                dev: dev_settings,
                explicit_path: explicit,
            },
        )?;
        let status = secrets::run_list(
            &root,
            Some(&secrets_bin),
            &env,
            &self.tenant,
            self.team.as_deref(),
        )?;
        if !status.success() {
            let code = status.code().unwrap_or(1);
            std::process::exit(code);
        }
        Ok(())
    }
}

impl DevSecretsDeleteArgs {
    fn run(self, settings: &crate::settings::OperatorSettings) -> anyhow::Result<()> {
        let root = super::project_root(self.project_root)?;
        let env = self.env.unwrap_or_else(secrets::resolve_env);
        let config = config::load_operator_config(&root)?;
        let dev_settings =
            super::resolve_dev_settings(settings, config.as_ref(), &self.dev, &root)?;
        let explicit = self
            .secrets_bin
            .clone()
            .or_else(|| config::binary_override(config.as_ref(), "greentic-secrets", &root));
        let secrets_bin = bin_resolver::resolve_binary(
            "greentic-secrets",
            &ResolveCtx {
                config_dir: root.clone(),
                dev: dev_settings,
                explicit_path: explicit,
            },
        )?;
        let status = secrets::run_delete(
            &root,
            Some(&secrets_bin),
            &env,
            &self.tenant,
            self.team.as_deref(),
            &self.key,
        )?;
        if !status.success() {
            let code = status.code().unwrap_or(1);
            std::process::exit(code);
        }
        Ok(())
    }
}
