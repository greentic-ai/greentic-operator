use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde_json::{Map as JsonMap, Value as JsonValue};

use clap::{Parser, Subcommand, ValueEnum};
use greentic_runner_desktop::RunStatus;

use crate::bin_resolver::{self, ResolveCtx};
use crate::config;
use crate::demo::setup::{ProvidersInput, discover_tenants};
use crate::demo::{self, BuildOptions};
use crate::dev_mode::{
    DevCliOverrides, DevMode, DevProfile, DevSettingsResolved, effective_dev_settings,
    merge_settings,
};
use crate::discovery;
use crate::domains::{self, Domain, DomainAction};
use crate::gmap::{self, Policy};
use crate::project::{self, ScanFormat};
use crate::runner_exec;
use crate::runner_integration;
use crate::runtime_state::RuntimePaths;
use crate::secrets_gate::{self, DynSecretsManager};
use crate::settings;
use crate::setup_input::{SetupInputAnswers, collect_setup_answers, load_setup_input};
use crate::state_layout;

mod dev_mode_cmd;
mod dev_secrets;

use dev_mode_cmd::{
    DevModeDetectArgs, DevModeMapCommand, DevModeOffArgs, DevModeOnArgs, DevModeStatusArgs,
};
use dev_secrets::DevSecretsCommand;
#[derive(Parser)]
#[command(name = "greentic-operator")]
#[command(about = "Greentic operator tooling", version)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Dev(DevCommand),
    Demo(DemoCommand),
}

#[derive(Parser)]
struct DevCommand {
    #[command(subcommand)]
    command: DevSubcommand,
}

#[derive(Parser)]
struct DemoCommand {
    #[arg(long, global = true)]
    debug: bool,
    #[command(subcommand)]
    command: DemoSubcommand,
}

#[derive(Subcommand)]
enum DevSubcommand {
    On(DevModeOnArgs),
    Off(DevModeOffArgs),
    Status(DevModeStatusArgs),
    Detect(DevModeDetectArgs),
    Map(DevModeMapCommand),
    Init(DevInitArgs),
    Scan(DevScanArgs),
    Sync(DevSyncArgs),
    Tenant(TenantCommand),
    Team(TeamCommand),
    Allow(DevPolicyArgs),
    Forbid(DevPolicyArgs),
    Up(DevUpArgs),
    Embedded(DevEmbeddedArgs),
    Down(DevDownArgs),
    #[command(name = "svc-status")]
    SvcStatus(DevStatusArgs),
    Logs(DevLogsArgs),
    Setup(DomainSetupArgs),
    Diagnostics(DomainDiagnosticsArgs),
    Verify(DomainVerifyArgs),
    Doctor(DevDoctorArgs),
    Secrets(DevSecretsCommand),
}

#[derive(Subcommand)]
enum DemoSubcommand {
    Build(DemoBuildArgs),
    Up(DemoUpArgs),
    Setup(DemoSetupArgs),
    Send(DemoSendArgs),
    Down(DemoDownArgs),
    Status(DemoStatusArgs),
    Logs(DemoLogsArgs),
    Doctor(DemoDoctorArgs),
    Allow(DemoPolicyArgs),
    Forbid(DemoPolicyArgs),
}

#[derive(Parser)]
#[command(
    about = "Initialize a new Greentic project layout.",
    long_about = "Creates the standard project directory layout, greentic.yaml, and the default tenant gmap.",
    after_help = "Main options:\n  (none)\n\nOptional options:\n  --project-root <PATH> (default: current directory)"
)]
struct DevInitArgs {
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "Scan providers, packs, tenants, and teams.",
    long_about = "Scans the project layout and prints a summary or structured output.",
    after_help = "Main options:\n  (none)\n\nOptional options:\n  --format <text|json|yaml> (default: text)\n  --project-root <PATH> (default: current directory)"
)]
struct DevScanArgs {
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "Generate resolved manifests for tenants and teams.",
    long_about = "Writes state/resolved/<tenant>[.<team>].yaml from discovered packs, providers, and gmaps.",
    after_help = "Main options:\n  (none)\n\nOptional options:\n  --project-root <PATH> (default: current directory)"
)]
struct DevSyncArgs {
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
struct TenantCommand {
    #[command(subcommand)]
    command: TenantSubcommand,
}

#[derive(Subcommand)]
enum TenantSubcommand {
    Add(TenantAddArgs),
    Rm(TenantRmArgs),
    List(TenantListArgs),
}

#[derive(Parser)]
#[command(
    about = "Add a tenant and initialize its gmap.",
    long_about = "Creates tenants/<tenant>/tenant.gmap with a default policy.",
    after_help = "Main options:\n  <TENANT>\n\nOptional options:\n  --project-root <PATH> (default: current directory)"
)]
struct TenantAddArgs {
    tenant: String,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "Remove a tenant and its data.",
    long_about = "Deletes tenants/<tenant> and any team gmaps under it.",
    after_help = "Main options:\n  <TENANT>\n\nOptional options:\n  --project-root <PATH> (default: current directory)"
)]
struct TenantRmArgs {
    tenant: String,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "List tenants in the project.",
    long_about = "Reads tenants/ and prints tenant ids.",
    after_help = "Main options:\n  (none)\n\nOptional options:\n  --project-root <PATH> (default: current directory)"
)]
struct TenantListArgs {
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
struct TeamCommand {
    #[command(subcommand)]
    command: TeamSubcommand,
}

#[derive(Subcommand)]
enum TeamSubcommand {
    Add(TeamAddArgs),
    Rm(TeamRmArgs),
    List(TeamListArgs),
}

#[derive(Parser)]
#[command(
    about = "Add a team under a tenant and initialize its gmap.",
    long_about = "Creates tenants/<tenant>/teams/<team>/team.gmap with a default policy.",
    after_help = "Main options:\n  <TEAM>\n  --tenant <TENANT>\n\nOptional options:\n  --project-root <PATH> (default: current directory)"
)]
struct TeamAddArgs {
    #[arg(long)]
    tenant: String,
    team: String,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "Remove a team from a tenant.",
    long_about = "Deletes tenants/<tenant>/teams/<team> and its gmap.",
    after_help = "Main options:\n  <TEAM>\n  --tenant <TENANT>\n\nOptional options:\n  --project-root <PATH> (default: current directory)"
)]
struct TeamRmArgs {
    #[arg(long)]
    tenant: String,
    team: String,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "List teams for a tenant.",
    long_about = "Reads tenants/<tenant>/teams and prints team ids.",
    after_help = "Main options:\n  --tenant <TENANT>\n\nOptional options:\n  --project-root <PATH> (default: current directory)"
)]
struct TeamListArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "Start local messaging services (and NATS unless disabled).",
    long_about = "Uses state/resolved/<tenant>[.<team>].yaml and launches greentic-messaging and optional NATS.",
    after_help = "Main options:\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --no-nats\n  --nats-url <URL>\n  --project-root <PATH> (default: current directory)\n  --dev-mode <auto|on|off>\n  --dev-root <PATH>\n  --dev-profile <debug|release>\n  --dev-target-dir <PATH>"
)]
struct DevUpArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    no_nats: bool,
    #[arg(long)]
    nats_url: Option<String>,
    #[arg(long)]
    project_root: Option<PathBuf>,
    #[command(flatten)]
    dev: DevModeArgs,
}

#[derive(Parser)]
#[command(
    about = "Run GSM gateway/egress/subscriptions in-process for local dev.",
    long_about = "Hosts the GSM services inside the operator process and blocks until Ctrl+C.",
    after_help = "Optional options:\n  --project-root <PATH> (default: current directory)\n  --no-nats"
)]
struct DevEmbeddedArgs {
    #[arg(long)]
    project_root: Option<PathBuf>,
    #[arg(long)]
    no_nats: bool,
}

#[derive(Parser)]
#[command(
    about = "Stop local messaging services (and NATS unless disabled).",
    long_about = "Stops services started by dev up and removes pidfiles.",
    after_help = "Main options:\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --no-nats\n  --project-root <PATH> (default: current directory)"
)]
struct DevDownArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    no_nats: bool,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "Show running status for local messaging services.",
    long_about = "Checks pidfiles for messaging and optional NATS.",
    after_help = "Main options:\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --no-nats\n  --project-root <PATH> (default: current directory)"
)]
struct DevStatusArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    no_nats: bool,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "Tail logs for messaging or NATS.",
    long_about = "Streams the latest log output for the selected service.",
    after_help = "Main options:\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --service <messaging|nats>\n  --project-root <PATH> (default: current directory)"
)]
struct DevLogsArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    service: Option<LogService>,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "Run provider setup flows for a domain.",
    long_about = "Executes setup_default across providers and can auto-run secrets init for messaging.",
    after_help = "Main options:\n  <DOMAIN>\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --provider <FILTER>\n  --dry-run\n  --format <text|json|yaml> (default: text)\n  --parallel <N> (default: 1)\n  --allow-missing-setup\n  --online\n  --secrets-env <ENV>\n  --secrets-bin <PATH>\n  --project-root <PATH> (default: current directory)"
)]
struct DomainSetupArgs {
    domain: DomainArg,
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    provider: Option<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, value_enum, default_value_t = PlanFormat::Text)]
    format: PlanFormat,
    #[arg(long, default_value_t = 1)]
    parallel: usize,
    #[arg(long)]
    allow_missing_setup: bool,
    #[arg(long)]
    online: bool,
    #[arg(long)]
    secrets_env: Option<String>,
    #[arg(long)]
    secrets_bin: Option<PathBuf>,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "Run provider diagnostics flows for a domain.",
    long_about = "Executes diagnostics for each provider pack that defines it.",
    after_help = "Main options:\n  <DOMAIN>\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --provider <FILTER>\n  --dry-run\n  --format <text|json|yaml> (default: text)\n  --parallel <N> (default: 1)\n  --online\n  --project-root <PATH> (default: current directory)"
)]
struct DomainDiagnosticsArgs {
    domain: DomainArg,
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    provider: Option<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, value_enum, default_value_t = PlanFormat::Text)]
    format: PlanFormat,
    #[arg(long, default_value_t = 1)]
    parallel: usize,
    #[arg(long)]
    online: bool,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    about = "Run provider verify flows for a domain.",
    long_about = "Executes verify_* flows where available for the selected domain.",
    after_help = "Main options:\n  <DOMAIN>\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --provider <FILTER>\n  --dry-run\n  --format <text|json|yaml> (default: text)\n  --parallel <N> (default: 1)\n  --online\n  --project-root <PATH> (default: current directory)"
)]
struct DomainVerifyArgs {
    domain: DomainArg,
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    provider: Option<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, value_enum, default_value_t = PlanFormat::Text)]
    format: PlanFormat,
    #[arg(long, default_value_t = 1)]
    parallel: usize,
    #[arg(long)]
    online: bool,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DomainArg {
    Messaging,
    Events,
    Secrets,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DoctorDomainArg {
    Messaging,
    Events,
    Secrets,
    All,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PlanFormat {
    Text,
    Json,
    Yaml,
}

#[derive(Parser)]
#[command(
    about = "Run domain doctor validation.",
    long_about = "Executes greentic-pack doctor with optional validators.",
    after_help = "Main options:\n  <DOMAIN>\n\nOptional options:\n  --tenant <TENANT>\n  --team <TEAM>\n  --strict\n  --validator-pack <PATH>...\n  --project-root <PATH> (default: current directory)\n  --dev-mode <auto|on|off>\n  --dev-root <PATH>\n  --dev-profile <debug|release>\n  --dev-target-dir <PATH>"
)]
struct DevDoctorArgs {
    domain: DoctorDomainArg,
    #[arg(long)]
    tenant: Option<String>,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    strict: bool,
    #[arg(long)]
    validator_pack: Vec<PathBuf>,
    #[arg(long)]
    project_root: Option<PathBuf>,
    #[command(flatten)]
    dev: DevModeArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum LogService {
    Messaging,
    Nats,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CloudflaredModeArg {
    On,
    Off,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RestartTarget {
    All,
    Cloudflared,
    Nats,
    Gateway,
    Egress,
    Subscriptions,
}

#[derive(Parser)]
#[command(
    about = "Build a portable demo bundle.",
    long_about = "Copies packs/providers/tenants and writes resolved manifests under the output directory.",
    after_help = "Main options:\n  --out <DIR>\n\nOptional options:\n  --tenant <TENANT>\n  --team <TEAM>\n  --allow-pack-dirs\n  --only-used-providers\n  --doctor\n  --project-root <PATH> (default: current directory)\n  --dev-mode <auto|on|off>\n  --dev-root <PATH>\n  --dev-profile <debug|release>\n  --dev-target-dir <PATH>"
)]
struct DemoBuildArgs {
    #[arg(long)]
    out: PathBuf,
    #[arg(long)]
    tenant: Option<String>,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    allow_pack_dirs: bool,
    #[arg(long)]
    only_used_providers: bool,
    #[arg(long)]
    doctor: bool,
    #[arg(long)]
    project_root: Option<PathBuf>,
    #[command(flatten)]
    dev: DevModeArgs,
}

#[derive(Parser)]
#[command(
    about = "Start demo services from a bundle.",
    long_about = "Uses resolved manifests inside the bundle to start services and optional NATS."
)]
struct DemoUpArgs {
    #[arg(
        long,
        help_heading = "Main options",
        help = "Path to the bundle directory to run in bundle mode."
    )]
    bundle: Option<PathBuf>,
    #[arg(
        long,
        default_value = "messaging",
        help_heading = "Optional options",
        help = "Domain(s) to operate on (messaging, events, secrets, all)."
    )]
    domain: DemoSetupDomainArg,
    #[arg(
        long,
        help_heading = "Main options",
        help = "JSON/YAML file describing provider setup inputs."
    )]
    setup_input: Option<PathBuf>,
    #[arg(
        long,
        help_heading = "Main options",
        help = "Optional override for the public base URL injected into every setup input."
    )]
    public_base_url: Option<String>,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "Tenant to target when running the bundle (defaults to demo)."
    )]
    tenant: Option<String>,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "Team to assign when running demo services."
    )]
    team: Option<String>,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "Skip spawning local NATS (used when you already have one)."
    )]
    no_nats: bool,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "URL of an existing NATS server to use instead of spawning one."
    )]
    nats_url: Option<String>,
    #[arg(
        long,
        default_value = "demo",
        help_heading = "Optional options",
        help = "Environment used for secrets lookups."
    )]
    env: String,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "Path to a prebuilt config file to use instead of auto-discovery."
    )]
    config: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = CloudflaredModeArg::On, help_heading = "Optional options", help = "Whether to start cloudflared for webhook tunneling.")]
    cloudflared: CloudflaredModeArg,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "Explicit path to the cloudflared binary used when cloudflared mode is on."
    )]
    cloudflared_binary: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        help_heading = "Optional options",
        help = "Comma-separated list of services to restart before running demo (e.g. gateway)."
    )]
    restart: Vec<RestartTarget>,
    #[arg(
        long,
        value_delimiter = ',',
        help_heading = "Optional options",
        help = "CSV list of provider pack IDs to restrict setup to."
    )]
    providers: Vec<String>,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "Avoid running provider setup flows."
    )]
    skip_setup: bool,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "Skip greentic-secrets init during setup."
    )]
    skip_secrets_init: bool,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "Run webhook verification flows after setup completes."
    )]
    verify_webhooks: bool,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "Force re-run of setup flows even if records already exist."
    )]
    force_setup: bool,
    #[arg(
        long,
        help_heading = "Optional options",
        help = "Path to a greentic-runner binary override."
    )]
    runner_binary: Option<PathBuf>,
    #[command(flatten)]
    dev: DevModeArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DemoSetupDomainArg {
    Messaging,
    Events,
    Secrets,
    All,
}

impl DemoSetupDomainArg {
    fn resolve_domains(self, discovery: Option<&discovery::DiscoveryResult>) -> Vec<Domain> {
        match self {
            DemoSetupDomainArg::Messaging => vec![Domain::Messaging],
            DemoSetupDomainArg::Events => vec![Domain::Events],
            DemoSetupDomainArg::Secrets => vec![Domain::Secrets],
            DemoSetupDomainArg::All => {
                let mut enabled = Vec::new();
                let has_messaging = discovery
                    .map(|value| value.domains.messaging)
                    .unwrap_or(true);
                let has_events = discovery.map(|value| value.domains.events).unwrap_or(true);
                if has_messaging {
                    enabled.push(Domain::Messaging);
                }
                if has_events {
                    enabled.push(Domain::Events);
                }
                enabled.push(Domain::Secrets);
                enabled
            }
        }
    }
}

#[derive(Parser)]
#[command(
    about = "Run provider setup flows against a demo bundle.",
    long_about = "Executes setup flows for provider packs included in the bundle.",
    after_help = "Main options:\n  --bundle <DIR>\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --domain <messaging|events|secrets|all> (default: all)\n  --provider <FILTER>\n  --dry-run\n  --format <text|json|yaml> (default: text)\n  --parallel <N> (default: 1)\n  --allow-missing-setup\n  --online\n  --secrets-env <ENV>\n  --secrets-bin <PATH>\n  --skip-secrets-init\n  --setup-input <PATH>\n  --runner-binary <PATH>\n  --best-effort"
)]
struct DemoSetupArgs {
    #[arg(long)]
    bundle: PathBuf,
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long, value_enum, default_value_t = DemoSetupDomainArg::All)]
    domain: DemoSetupDomainArg,
    #[arg(long)]
    provider: Option<String>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    #[arg(long, default_value_t = 1)]
    parallel: usize,
    #[arg(long)]
    allow_missing_setup: bool,
    #[arg(long)]
    online: bool,
    #[arg(long)]
    secrets_env: Option<String>,
    #[arg(long)]
    secrets_bin: Option<PathBuf>,
    #[arg(long)]
    skip_secrets_init: bool,
    #[arg(long)]
    state_dir: Option<PathBuf>,
    #[arg(long)]
    runner_binary: Option<PathBuf>,
    #[arg(long)]
    setup_input: Option<PathBuf>,
    #[arg(long)]
    best_effort: bool,
}

#[derive(Parser)]
#[command(
    about = "Allow/forbid a gmap rule inside a demo bundle.",
    long_about = "Updates the demo bundle's gmap, reruns the resolver, and copies the updated manifest so demo up sees the change immediately.",
    after_help = "Main options:\n  --bundle <DIR>\n  --tenant <TENANT>\n  --path <PACK[/FLOW[/NODE]] (up to 3 segments)\n\nOptional options:\n  --team <TEAM>\n\nPaths use the same PACK[/FLOW[/NODE]] syntax as the dev allow/forbid commands (max 3 segments). The command modifies tenants/<tenant>[/teams/<team>]/(tenant|team).gmap, resolves state/resolved/<tenant>[.<team>].yaml, and overwrites resolved/<tenant>[.<team>].yaml so demo up picks it up without a rebuild."
)]
struct DemoPolicyArgs {
    #[arg(long, help = "Path to the demo bundle directory.")]
    bundle: PathBuf,
    #[arg(long, help = "Tenant owning the gmap rule.")]
    tenant: String,
    #[arg(long, help = "Team owning the gmap rule.")]
    team: Option<String>,
    #[arg(long, help = "Gmap path to allow or forbid.")]
    path: String,
}
#[derive(Parser)]
#[command(
    about = "Stop demo services using runtime state.",
    long_about = "Stops services recorded in state/pids for a tenant/team or all services when --all is set.",
    after_help = "Main options:\n  (none)\n\nOptional options:\n  --tenant <TENANT> (default: demo)\n  --team <TEAM> (default: default)\n  --state-dir <PATH> (default: ./state or <bundle>/state)\n  --bundle <DIR> (legacy mode if --state-dir omitted)\n  --all\n  --verbose\n  --no-nats"
)]
struct DemoDownArgs {
    #[arg(long)]
    bundle: Option<PathBuf>,
    #[arg(long, default_value = "demo")]
    tenant: String,
    #[arg(long, default_value = "default")]
    team: String,
    #[arg(long)]
    state_dir: Option<PathBuf>,
    #[arg(long)]
    all: bool,
    #[arg(long)]
    verbose: bool,
    #[arg(long)]
    no_nats: bool,
}

#[derive(Parser)]
#[command(
    about = "Show demo service status using runtime state.",
    long_about = "Lists pidfiles under state/pids for the selected tenant/team.",
    after_help = "Main options:\n  (none)\n\nOptional options:\n  --tenant <TENANT> (default: demo)\n  --team <TEAM> (default: default)\n  --state-dir <PATH> (default: ./state or <bundle>/state)\n  --bundle <DIR> (legacy mode if --state-dir omitted)\n  --verbose\n  --no-nats"
)]
struct DemoStatusArgs {
    #[arg(long)]
    bundle: Option<PathBuf>,
    #[arg(long, default_value = "demo")]
    tenant: String,
    #[arg(long, default_value = "default")]
    team: String,
    #[arg(long)]
    state_dir: Option<PathBuf>,
    #[arg(long)]
    verbose: bool,
    #[arg(long)]
    no_nats: bool,
}

#[derive(Parser)]
#[command(
    about = "Show demo service logs using runtime state.",
    long_about = "Prints or tails logs under state/logs for the selected service.",
    after_help = "Main options:\n  <SERVICE> (messaging|nats|cloudflared)\n\nOptional options:\n  --tail\n  --tenant <TENANT> (default: demo)\n  --team <TEAM> (default: default)\n  --state-dir <PATH> (default: ./state or <bundle>/state)\n  --bundle <DIR> (legacy mode if --state-dir omitted)\n  --verbose\n  --no-nats"
)]
struct DemoLogsArgs {
    #[arg(default_value = "messaging")]
    service: String,
    #[arg(long)]
    tail: bool,
    #[arg(long)]
    bundle: Option<PathBuf>,
    #[arg(long, default_value = "demo")]
    tenant: String,
    #[arg(long, default_value = "default")]
    team: String,
    #[arg(long)]
    state_dir: Option<PathBuf>,
    #[arg(long)]
    verbose: bool,
    #[arg(long)]
    no_nats: bool,
}

#[derive(Parser)]
#[command(
    about = "Run demo doctor validation from a bundle.",
    long_about = "Runs greentic-pack doctor against packs in the demo bundle.",
    after_help = "Main options:\n  --bundle <DIR>\n\nOptional options:\n  --dev-mode <auto|on|off>\n  --dev-root <PATH>\n  --dev-profile <debug|release>\n  --dev-target-dir <PATH>"
)]
struct DemoDoctorArgs {
    #[arg(long)]
    bundle: PathBuf,
    #[command(flatten)]
    dev: DevModeArgs,
}

#[derive(Parser)]
#[command(
    about = "Send a demo message via a provider pack.",
    long_about = "Runs provider requirements or sends a generic message payload.",
    after_help = "Main options:\n  --bundle <DIR>\n  --provider <PROVIDER>\n\nOptional options:\n  --text <TEXT>\n  --arg <k=v>...\n  --args-json <JSON>\n  --tenant <TENANT> (default: demo)\n  --team <TEAM> (default: default)\n  --print-required-args"
)]
struct DemoSendArgs {
    #[arg(long)]
    bundle: PathBuf,
    #[arg(long)]
    provider: String,
    #[arg(long)]
    text: Option<String>,
    #[arg(long = "arg")]
    args: Vec<String>,
    #[arg(long)]
    args_json: Option<String>,
    #[arg(long, default_value = "demo")]
    tenant: String,
    #[arg(long, default_value = "default")]
    team: String,
    #[arg(long)]
    print_required_args: bool,
}
#[derive(Parser)]
#[command(
    about = "Allow/forbid a gmap rule for tenant or team.",
    long_about = "Updates the appropriate gmap file with a deterministic ordering.",
    after_help = "Main options:\n  --tenant <TENANT>\n  --path <PACK[/FLOW[/NODE]]>\n\nOptional options:\n  --team <TEAM>\n  --project-root <PATH> (default: current directory)"
)]
struct DevPolicyArgs {
    #[arg(long)]
    tenant: String,
    #[arg(long)]
    team: Option<String>,
    #[arg(long)]
    path: String,
    #[arg(long)]
    project_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Parser)]
struct DevModeArgs {
    #[arg(long, value_enum)]
    dev_mode: Option<DevModeArg>,
    #[arg(long)]
    dev_root: Option<PathBuf>,
    #[arg(long, value_enum)]
    dev_profile: Option<DevProfileArg>,
    #[arg(long)]
    dev_target_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DevModeArg {
    Auto,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DevProfileArg {
    Debug,
    Release,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Format {
    Text,
    Json,
    Yaml,
}

impl Cli {
    pub fn run(self) -> anyhow::Result<()> {
        let settings = settings::load_settings()?;
        let mut ctx = AppCtx { settings };
        match self.command {
            Command::Dev(dev) => dev.run(&mut ctx),
            Command::Demo(demo) => demo.run(&ctx),
        }
    }
}

struct AppCtx {
    settings: settings::OperatorSettings,
}

impl DevCommand {
    fn run(self, ctx: &mut AppCtx) -> anyhow::Result<()> {
        match self.command {
            DevSubcommand::On(args) => {
                ctx.settings = args.run(ctx.settings.clone())?;
                Ok(())
            }
            DevSubcommand::Off(args) => {
                ctx.settings = args.run(ctx.settings.clone())?;
                Ok(())
            }
            DevSubcommand::Status(args) => {
                ctx.settings = args.run(ctx.settings.clone())?;
                Ok(())
            }
            DevSubcommand::Detect(args) => {
                ctx.settings = args.run(ctx.settings.clone())?;
                Ok(())
            }
            DevSubcommand::Map(args) => {
                ctx.settings = args.run(ctx.settings.clone())?;
                Ok(())
            }
            DevSubcommand::Init(args) => args.run(),
            DevSubcommand::Scan(args) => args.run(),
            DevSubcommand::Sync(args) => args.run(),
            DevSubcommand::Tenant(args) => args.run(),
            DevSubcommand::Team(args) => args.run(),
            DevSubcommand::Allow(args) => args.run(Policy::Public),
            DevSubcommand::Forbid(args) => args.run(Policy::Forbidden),
            DevSubcommand::Up(args) => args.run(ctx),
            DevSubcommand::Embedded(args) => args.run(ctx),
            DevSubcommand::Down(args) => args.run(),
            DevSubcommand::SvcStatus(args) => args.run(),
            DevSubcommand::Logs(args) => args.run(),
            DevSubcommand::Setup(args) => args.run(),
            DevSubcommand::Diagnostics(args) => args.run(),
            DevSubcommand::Verify(args) => args.run(),
            DevSubcommand::Doctor(args) => args.run(ctx),
            DevSubcommand::Secrets(args) => args.run(&ctx.settings),
        }
    }
}

impl DemoCommand {
    fn run(self, ctx: &AppCtx) -> anyhow::Result<()> {
        if self.debug {
            unsafe {
                std::env::set_var("GREENTIC_OPERATOR_DEMO_DEBUG", "1");
            }
        }
        match self.command {
            DemoSubcommand::Build(args) => args.run(ctx),
            DemoSubcommand::Up(args) => args.run(ctx),
            DemoSubcommand::Setup(args) => args.run(),
            DemoSubcommand::Send(args) => args.run(),
            DemoSubcommand::Down(args) => args.run(),
            DemoSubcommand::Status(args) => args.run(),
            DemoSubcommand::Logs(args) => args.run(),
            DemoSubcommand::Doctor(args) => args.run(ctx),
            DemoSubcommand::Allow(args) => args.run(Policy::Public),
            DemoSubcommand::Forbid(args) => args.run(Policy::Forbidden),
        }
    }
}

impl DevInitArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        project::init_project(&root)
    }
}

impl DevScanArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        let format = match self.format {
            Format::Text => ScanFormat::Text,
            Format::Json => ScanFormat::Json,
            Format::Yaml => ScanFormat::Yaml,
        };
        project::scan_project(&root, format)
    }
}

impl DevSyncArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        project::sync_project(&root)
    }
}

impl TenantCommand {
    fn run(self) -> anyhow::Result<()> {
        match self.command {
            TenantSubcommand::Add(args) => args.run(),
            TenantSubcommand::Rm(args) => args.run(),
            TenantSubcommand::List(args) => args.run(),
        }
    }
}

impl TenantAddArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        project::add_tenant(&root, &self.tenant)
    }
}

impl TenantRmArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        project::remove_tenant(&root, &self.tenant)
    }
}

impl TenantListArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        for tenant in project::list_tenants(&root)? {
            println!("{tenant}");
        }
        Ok(())
    }
}

impl TeamCommand {
    fn run(self) -> anyhow::Result<()> {
        match self.command {
            TeamSubcommand::Add(args) => args.run(),
            TeamSubcommand::Rm(args) => args.run(),
            TeamSubcommand::List(args) => args.run(),
        }
    }
}

impl TeamAddArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        project::add_team(&root, &self.tenant, &self.team)
    }
}

impl TeamRmArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        project::remove_team(&root, &self.tenant, &self.team)
    }
}

impl TeamListArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        for team in project::list_teams(&root, &self.tenant)? {
            println!("{team}");
        }
        Ok(())
    }
}

impl DevPolicyArgs {
    fn run(self, policy: Policy) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        let gmap_path = match self.team {
            Some(team) => root
                .join("tenants")
                .join(&self.tenant)
                .join("teams")
                .join(team)
                .join("team.gmap"),
            None => root.join("tenants").join(&self.tenant).join("tenant.gmap"),
        };
        gmap::upsert_policy(&gmap_path, &self.path, policy)
    }
}

impl DevUpArgs {
    fn run(self, ctx: &AppCtx) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        let config = config::load_operator_config(&root)?;
        let dev_settings = resolve_dev_settings(&ctx.settings, config.as_ref(), &self.dev, &root)?;
        let discovery = discovery::discover(&root)?;
        discovery::persist(&root, &self.tenant, &discovery)?;
        let services = config
            .as_ref()
            .and_then(|config| config.services.clone())
            .unwrap_or_default();
        let messaging_enabled = services
            .messaging
            .enabled
            .is_enabled(discovery.domains.messaging);
        let events_enabled = services.events.enabled.is_enabled(discovery.domains.events);

        let mut nats_url = self.nats_url.clone();
        let mut nats_started = false;
        if !self.no_nats && nats_url.is_none() && (messaging_enabled || events_enabled) {
            if let Err(err) = crate::services::start_nats(&root) {
                eprintln!("Warning: failed to start NATS: {err}");
            } else {
                nats_url = Some(crate::services::nats_url(&root));
                nats_started = true;
            }
        }

        let mut event_specs = Vec::new();
        if events_enabled {
            let envs = build_domain_env(&self.tenant, self.team.as_deref(), nats_url.as_deref());
            for spec in resolve_event_components(&root, config.as_ref(), &services, &dev_settings)?
            {
                let state = crate::services::start_component(&root, &spec, &envs)?;
                println!("{}: {:?}", spec.id, state);
                event_specs.push(spec);
            }
        } else {
            println!("events: skipped (disabled or no providers)");
        }

        if messaging_enabled {
            crate::services::run_services(&root)?;
        } else {
            println!("messaging: skipped (disabled or no providers)");
        }

        for spec in event_specs {
            let id = spec.id.clone();
            let state = crate::services::stop_component(&root, &id)?;
            println!("{id}: {:?}", state);
        }

        if nats_started {
            let nats = crate::services::stop_nats(&root)?;
            println!("nats: {:?}", nats);
        }

        Ok(())
    }
}

impl DevEmbeddedArgs {
    fn run(self, _ctx: &AppCtx) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        let mut nats_started = false;
        if !self.no_nats {
            if let Err(err) = crate::services::start_nats(&root) {
                eprintln!("Warning: failed to start NATS: {err}");
            } else {
                nats_started = true;
            }
        }

        crate::services::run_services(&root)?;

        if nats_started {
            let nats = crate::services::stop_nats(&root)?;
            println!("nats: {:?}", nats);
        }

        Ok(())
    }
}

impl DevDownArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        let config = config::load_operator_config(&root)?;
        let services = config
            .as_ref()
            .and_then(|config| config.services.clone())
            .unwrap_or_default();
        println!(
            "messaging: embedded (starts/stops with `dev up`/`dev embedded`; Ctrl+C in those commands to exit)"
        );

        for id in event_component_ids(&services) {
            let state = crate::services::stop_component(&root, &id)?;
            println!("{id}: {:?}", state);
        }

        if !self.no_nats {
            let nats = crate::services::stop_nats(&root)?;
            println!("nats: {:?}", nats);
        }
        Ok(())
    }
}

impl DevStatusArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        let config = config::load_operator_config(&root)?;
        let discovery = discovery::discover(&root)?;
        discovery::persist(&root, &self.tenant, &discovery)?;
        println!(
            "detected domains: messaging={} events={}",
            discovery.domains.messaging, discovery.domains.events
        );
        for provider in &discovery.providers {
            println!(
                "detected provider: {} ({} via {:?})",
                provider.provider_id, provider.domain, provider.id_source
            );
        }
        let services = config
            .as_ref()
            .and_then(|config| config.services.clone())
            .unwrap_or_default();
        let messaging_enabled = services
            .messaging
            .enabled
            .is_enabled(discovery.domains.messaging);
        let events_enabled = services.events.enabled.is_enabled(discovery.domains.events);

        if messaging_enabled {
            println!(
                "messaging: embedded (starts/stops with `dev up`/`dev embedded`; Ctrl+C to exit)"
            );
        } else {
            println!("messaging: skipped (disabled or no providers)");
        }

        if events_enabled {
            for id in event_component_ids(&services) {
                let status = crate::services::component_status(&root, &id)?;
                println!("{id}: {:?}", status);
            }
        } else {
            println!("events: skipped (disabled or no providers)");
        }

        if !self.no_nats {
            let nats = crate::services::nats_status(&root)?;
            println!("nats: {:?}", nats);
        }
        Ok(())
    }
}

impl DevLogsArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        match self.service.unwrap_or(LogService::Messaging) {
            LogService::Messaging => {
                println!(
                    "messaging: logs are streamed to this process while `dev up`/`dev embedded` run; rerun those commands to see output."
                );
                Ok(())
            }
            LogService::Nats => crate::services::tail_nats_logs(&root),
        }
    }
}

impl DomainSetupArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        run_domain_command(DomainRunArgs {
            root,
            state_root: None,
            domain: self.domain.into(),
            action: DomainAction::Setup,
            tenant: self.tenant,
            team: self.team,
            provider_filter: self.provider,
            dry_run: self.dry_run,
            format: self.format,
            parallel: self.parallel,
            allow_missing_setup: self.allow_missing_setup,
            online: self.online,
            secrets_env: self.secrets_env,
            secrets_bin: self.secrets_bin,
            runner_binary: None,
            best_effort: false,
            setup_input: None,
            allowed_providers: None,
            preloaded_setup_answers: None,
            public_base_url: None,
            secrets_manager: None,
            discovered_providers: None,
        })
    }
}

impl DomainDiagnosticsArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        run_domain_command(DomainRunArgs {
            root,
            state_root: None,
            domain: self.domain.into(),
            action: DomainAction::Diagnostics,
            tenant: self.tenant,
            team: self.team,
            provider_filter: self.provider,
            dry_run: self.dry_run,
            format: self.format,
            parallel: self.parallel,
            allow_missing_setup: true,
            online: self.online,
            secrets_env: None,
            secrets_bin: None,
            runner_binary: None,
            best_effort: false,
            setup_input: None,
            allowed_providers: None,
            preloaded_setup_answers: None,
            public_base_url: None,
            secrets_manager: None,
            discovered_providers: None,
        })
    }
}

impl DomainVerifyArgs {
    fn run(self) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        run_domain_command(DomainRunArgs {
            root,
            state_root: None,
            domain: self.domain.into(),
            action: DomainAction::Verify,
            tenant: self.tenant,
            team: self.team,
            provider_filter: self.provider,
            dry_run: self.dry_run,
            format: self.format,
            parallel: self.parallel,
            allow_missing_setup: true,
            online: self.online,
            secrets_env: None,
            secrets_bin: None,
            runner_binary: None,
            best_effort: false,
            setup_input: None,
            allowed_providers: None,
            preloaded_setup_answers: None,
            public_base_url: None,
            secrets_manager: None,
            discovered_providers: None,
        })
    }
}

impl DevDoctorArgs {
    fn run(self, ctx: &AppCtx) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        let config = config::load_operator_config(&root)?;
        let dev_settings = resolve_dev_settings(&ctx.settings, config.as_ref(), &self.dev, &root)?;
        let explicit = config::binary_override(config.as_ref(), "greentic-pack", &root);
        let pack_command = bin_resolver::resolve_binary(
            "greentic-pack",
            &ResolveCtx {
                config_dir: root.clone(),
                dev: dev_settings,
                explicit_path: explicit,
            },
        )?;

        let scope = match self.domain {
            DoctorDomainArg::Messaging => crate::doctor::DoctorScope::One(Domain::Messaging),
            DoctorDomainArg::Events => crate::doctor::DoctorScope::One(Domain::Events),
            DoctorDomainArg::Secrets => crate::doctor::DoctorScope::One(Domain::Secrets),
            DoctorDomainArg::All => crate::doctor::DoctorScope::All,
        };
        crate::doctor::run_doctor(
            &root,
            scope,
            crate::doctor::DoctorOptions {
                tenant: self.tenant,
                team: self.team,
                strict: self.strict,
                validator_packs: self.validator_pack,
            },
            &pack_command,
        )?;
        Ok(())
    }
}

impl DemoBuildArgs {
    fn run(self, ctx: &AppCtx) -> anyhow::Result<()> {
        let root = project_root(self.project_root)?;
        if demo_debug_enabled() {
            println!(
                "[demo] build root={} out={} tenant={:?} team={:?} doctor={}",
                root.display(),
                self.out.display(),
                self.tenant,
                self.team,
                self.doctor
            );
        }
        let options = BuildOptions {
            out_dir: self.out,
            tenant: self.tenant,
            team: self.team,
            allow_pack_dirs: self.allow_pack_dirs,
            only_used_providers: self.only_used_providers,
            run_doctor: self.doctor,
        };
        let config = config::load_operator_config(&root)?;
        let dev_settings = resolve_dev_settings(&ctx.settings, config.as_ref(), &self.dev, &root)?;
        let pack_command = if options.run_doctor {
            let explicit = config::binary_override(config.as_ref(), "greentic-pack", &root);
            Some(bin_resolver::resolve_binary(
                "greentic-pack",
                &ResolveCtx {
                    config_dir: root.clone(),
                    dev: dev_settings,
                    explicit_path: explicit,
                },
            )?)
        } else {
            None
        };
        demo::build_bundle(&root, options, pack_command.as_deref())
    }
}

impl DemoUpArgs {
    fn run(self, ctx: &AppCtx) -> anyhow::Result<()> {
        let restart: std::collections::BTreeSet<String> =
            self.restart.iter().map(restart_name).collect();
        if let Some(bundle) = self.bundle {
            if demo_debug_enabled() {
                println!(
                    "[demo] up bundle={} tenant={:?} team={:?} no_nats={} nats_url={:?} cloudflared={:?}",
                    bundle.display(),
                    self.tenant,
                    self.team,
                    self.no_nats,
                    self.nats_url,
                    self.cloudflared
                );
            }
            let tenant = self.tenant.clone().unwrap_or_else(|| "demo".to_string());
            let config = config::load_operator_config(&bundle)?;
            let dev_settings =
                resolve_dev_settings(&ctx.settings, config.as_ref(), &self.dev, &bundle)?;
            domains::ensure_cbor_packs(&bundle)?;
            let discovery = discovery::discover_with_options(
                &bundle,
                discovery::DiscoveryOptions { cbor_only: true },
            )?;
            discovery::persist(&bundle, &tenant, &discovery)?;
            let services = config
                .as_ref()
                .and_then(|config| config.services.clone())
                .unwrap_or_default();
            let messaging_enabled = services
                .messaging
                .enabled
                .is_enabled(discovery.domains.messaging);
            let events_enabled = services.events.enabled.is_enabled(discovery.domains.events);
            let events_components = if events_enabled {
                resolve_event_components(&bundle, config.as_ref(), &services, &dev_settings)?
            } else {
                Vec::new()
            };
            let domains_to_setup = self.domain.resolve_domains(Some(&discovery));

            let mut cloudflared_config = match self.cloudflared {
                CloudflaredModeArg::Off => None,
                CloudflaredModeArg::On => {
                    let explicit = self.cloudflared_binary.clone();
                    let binary = bin_resolver::resolve_binary(
                        "cloudflared",
                        &ResolveCtx {
                            config_dir: bundle.clone(),
                            dev: None,
                            explicit_path: explicit,
                        },
                    )?;
                    Some(crate::cloudflared::CloudflaredConfig {
                        binary,
                        local_port: 8080,
                        extra_args: Vec::new(),
                        restart: restart.contains("cloudflared"),
                    })
                }
            };

            let mut public_base_url = self.public_base_url.clone();
            let team_id = self.team.clone().unwrap_or_else(|| "default".to_string());
            let mut started_cloudflared_early = false;
            if public_base_url.is_none() && self.setup_input.is_some() {
                if let Some(cfg) = cloudflared_config.as_mut() {
                    let paths = RuntimePaths::new(&bundle.join("state"), &tenant, &team_id);
                    let handle = crate::cloudflared::start_quick_tunnel(&paths, cfg)?;
                    let domain_labels = domains_to_setup
                        .iter()
                        .map(|domain| domains::domain_name(*domain))
                        .collect::<Vec<_>>()
                        .join(",");
                    println!(
                        "Public URL (cloudflared setup domains={domain_labels}): {}",
                        handle.url
                    );
                    public_base_url = Some(handle.url.clone());
                    started_cloudflared_early = true;
                }
            }

            if started_cloudflared_early {
                if let Some(cfg) = cloudflared_config.as_mut() {
                    cfg.restart = false;
                }
            }

            if let Some(setup_input) = self.setup_input.as_ref() {
                let secrets_manager = secrets_gate::default_manager();
                run_demo_up_setup(
                    &bundle,
                    &domains_to_setup,
                    setup_input,
                    self.tenant.clone(),
                    self.team.clone(),
                    &self.env,
                    self.runner_binary.clone(),
                    public_base_url.clone(),
                    Some(secrets_manager),
                )?;
            }

            return demo::demo_up(
                &bundle,
                &tenant,
                self.team.as_deref(),
                self.nats_url.as_deref(),
                self.no_nats,
                messaging_enabled,
                cloudflared_config,
                events_components,
            );
        }

        let config_path = resolve_demo_config_path(self.config)?;
        let demo_config = config::load_demo_config(&config_path)?;
        let operator_config =
            config::load_operator_config(config_path.parent().unwrap_or(Path::new(".")))?;
        let dev_settings = resolve_dev_settings(
            &ctx.settings,
            operator_config.as_ref(),
            &self.dev,
            config_path.parent().unwrap_or(Path::new(".")),
        )?;
        let cloudflared = match self.cloudflared {
            CloudflaredModeArg::Off => None,
            CloudflaredModeArg::On => {
                let explicit = self.cloudflared_binary.clone();
                let binary = bin_resolver::resolve_binary(
                    "cloudflared",
                    &ResolveCtx {
                        config_dir: config_path.parent().unwrap_or(Path::new(".")).to_path_buf(),
                        dev: None,
                        explicit_path: explicit,
                    },
                )?;
                Some(crate::cloudflared::CloudflaredConfig {
                    binary,
                    local_port: demo_config.services.gateway.port,
                    extra_args: Vec::new(),
                    restart: restart.contains("cloudflared"),
                })
            }
        };

        let provider_options = crate::providers::ProviderSetupOptions {
            providers: if self.providers.is_empty() {
                None
            } else {
                Some(self.providers)
            },
            verify_webhooks: self.verify_webhooks,
            force_setup: self.force_setup,
            skip_setup: self.skip_setup,
            skip_secrets_init: self.skip_secrets_init,
            setup_input: self.setup_input,
            runner_binary: self.runner_binary,
        };

        demo::demo_up_services(
            &config_path,
            &demo_config,
            dev_settings,
            cloudflared,
            &restart,
            provider_options,
        )
    }
}

impl DemoSetupArgs {
    fn run(self) -> anyhow::Result<()> {
        domains::ensure_cbor_packs(&self.bundle)?;
        let discovery = discovery::discover_with_options(
            &self.bundle,
            discovery::DiscoveryOptions { cbor_only: true },
        )?;
        discovery::persist(&self.bundle, &self.tenant, &discovery)?;
        let domains = self.domain.resolve_domains(Some(&discovery));
        if demo_debug_enabled() {
            println!(
                "[demo] setup bundle={} tenant={} team={:?} domains={:?} provider_filter={:?} dry_run={} parallel={} skip_secrets_init={}",
                self.bundle.display(),
                self.tenant,
                self.team,
                domains,
                self.provider,
                self.dry_run,
                self.parallel,
                self.skip_secrets_init
            );
        }
        let format = match self.format {
            Format::Text => PlanFormat::Text,
            Format::Json => PlanFormat::Json,
            Format::Yaml => PlanFormat::Yaml,
        };
        for domain in domains {
            let discovered_providers = match domain {
                Domain::Messaging | Domain::Events => Some(
                    discovery
                        .providers
                        .iter()
                        .filter(|provider| provider.domain == domains::domain_name(domain))
                        .cloned()
                        .collect(),
                ),
                Domain::Secrets => None,
            };
            run_domain_command(DomainRunArgs {
                root: self.bundle.clone(),
                state_root: self.state_dir.clone(),
                domain,
                action: DomainAction::Setup,
                tenant: self.tenant.clone(),
                team: self.team.clone(),
                provider_filter: self.provider.clone(),
                dry_run: self.dry_run,
                format,
                parallel: self.parallel,
                allow_missing_setup: self.allow_missing_setup,
                online: self.online,
                secrets_env: if self.skip_secrets_init {
                    None
                } else {
                    self.secrets_env.clone()
                },
                secrets_bin: if self.skip_secrets_init {
                    None
                } else {
                    self.secrets_bin.clone()
                },
                runner_binary: self.runner_binary.clone(),
                best_effort: self.best_effort,
                setup_input: self.setup_input.clone(),
                allowed_providers: None,
                preloaded_setup_answers: None,
                public_base_url: None,
                secrets_manager: None,
                discovered_providers,
            })?;
        }
        Ok(())
    }
}

impl DemoPolicyArgs {
    fn run(self, policy: Policy) -> anyhow::Result<()> {
        let gmap_path = demo_bundle_gmap_path(&self.bundle, &self.tenant, self.team.as_deref());
        gmap::upsert_policy(&gmap_path, &self.path, policy)?;
        project::sync_project(&self.bundle)?;
        copy_resolved_manifest(&self.bundle, &self.tenant, self.team.as_deref())?;
        Ok(())
    }
}

impl DemoSendArgs {
    fn run(self) -> anyhow::Result<()> {
        let team = if self.team.is_empty() {
            None
        } else {
            Some(self.team.as_str())
        };
        domains::ensure_cbor_packs(&self.bundle)?;
        let pack = resolve_demo_provider_pack(
            &self.bundle,
            &self.tenant,
            team,
            &self.provider,
            Domain::Messaging,
        )?;
        let discovery = discovery::discover_with_options(
            &self.bundle,
            discovery::DiscoveryOptions { cbor_only: true },
        )?;
        let provider_map = discovery_map(&discovery.providers);
        let provider_id = provider_id_for_pack(&pack.path, &pack.pack_id, Some(&provider_map));

        if self.print_required_args {
            if let Err(message) = ensure_requirements_flow(&pack) {
                eprintln!("{message}");
                std::process::exit(2);
            }
            let env = crate::tools::secrets::resolve_env();
            let input = build_input_payload(
                &self.bundle,
                Domain::Messaging,
                &self.tenant,
                team,
                Some(&pack.pack_id),
                None,
                None,
                &env,
            );
            let outcome = run_demo_flow(
                &self.bundle,
                Domain::Messaging,
                &self.tenant,
                team,
                &pack,
                "requirements",
                &input,
                None,
            )?;
            if !outcome.success {
                let message = outcome
                    .error
                    .unwrap_or_else(|| "requirements flow failed".to_string());
                return Err(anyhow::anyhow!(message));
            }
            if let Some(value) = outcome.output {
                if let Some(rendered) = format_requirements_output(&value) {
                    println!("{rendered}");
                } else {
                    let json = serde_json::to_string_pretty(&value)?;
                    println!("{json}");
                }
            } else if let Some(raw) = outcome.raw {
                println!("{raw}");
            }
            return Ok(());
        }

        let text = self
            .text
            .clone()
            .ok_or_else(|| anyhow::anyhow!("--text is required unless --print-required-args"))?;
        let args = merge_args(self.args_json.as_deref(), &self.args)?;
        let mut payload = serde_json::json!({
            "tenant": self.tenant,
            "provider": provider_id,
            "text": text,
            "args": JsonValue::Object(args),
        });
        if let Some(team) = team {
            payload["team"] = JsonValue::String(team.to_string());
        }

        let outcome = run_demo_flow(
            &self.bundle,
            Domain::Messaging,
            &self.tenant,
            team,
            &pack,
            "send",
            &payload,
            None,
        )?;
        if outcome.success {
            println!("ok");
            if let Some(value) = outcome.output {
                let json = serde_json::to_string_pretty(&value)?;
                println!("{json}");
            } else if let Some(raw) = outcome.raw {
                println!("{raw}");
            }
            return Ok(());
        }
        let message = outcome.error.unwrap_or_else(|| "send failed".to_string());
        Err(anyhow::anyhow!(message))
    }
}
fn restart_name(target: &RestartTarget) -> String {
    match target {
        RestartTarget::All => "all",
        RestartTarget::Cloudflared => "cloudflared",
        RestartTarget::Nats => "nats",
        RestartTarget::Gateway => "gateway",
        RestartTarget::Egress => "egress",
        RestartTarget::Subscriptions => "subscriptions",
    }
    .to_string()
}

fn resolve_demo_config_path(explicit: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let cwd = std::env::current_dir()?;
    let demo_path = cwd.join("demo").join("demo.yaml");
    if demo_path.exists() {
        return Ok(demo_path);
    }
    let fallback = cwd.join("greentic.operator.yaml");
    if fallback.exists() {
        return Ok(fallback);
    }
    Err(anyhow::anyhow!(
        "no demo config found; pass --config or create ./demo/demo.yaml"
    ))
}

impl DemoDownArgs {
    fn run(self) -> anyhow::Result<()> {
        let state_dir = resolve_state_dir(self.state_dir, self.bundle.as_ref());
        if demo_debug_enabled() {
            println!(
                "[demo] down state_dir={} tenant={} team={} all={}",
                state_dir.display(),
                self.tenant,
                self.team,
                self.all
            );
        }
        demo::demo_down_runtime(&state_dir, &self.tenant, &self.team, self.all)
    }
}

impl DemoStatusArgs {
    fn run(self) -> anyhow::Result<()> {
        let state_dir = resolve_state_dir(self.state_dir, self.bundle.as_ref());
        if demo_debug_enabled() {
            println!(
                "[demo] status state_dir={} tenant={} team={} verbose={}",
                state_dir.display(),
                self.tenant,
                self.team,
                self.verbose
            );
        }
        demo::demo_status_runtime(&state_dir, &self.tenant, &self.team, self.verbose)
    }
}

impl DemoLogsArgs {
    fn run(self) -> anyhow::Result<()> {
        let state_dir = resolve_state_dir(self.state_dir, self.bundle.as_ref());
        if demo_debug_enabled() {
            println!(
                "[demo] logs state_dir={} tenant={} team={} service={} tail={}",
                state_dir.display(),
                self.tenant,
                self.team,
                self.service,
                self.tail
            );
        }
        demo::demo_logs_runtime(
            &state_dir,
            &self.tenant,
            &self.team,
            &self.service,
            self.tail,
        )
    }
}

impl DemoDoctorArgs {
    fn run(self, ctx: &AppCtx) -> anyhow::Result<()> {
        let config = config::load_operator_config(&self.bundle)?;
        let dev_settings =
            resolve_dev_settings(&ctx.settings, config.as_ref(), &self.dev, &self.bundle)?;
        let explicit = config::binary_override(config.as_ref(), "greentic-pack", &self.bundle);
        let pack_command = bin_resolver::resolve_binary(
            "greentic-pack",
            &ResolveCtx {
                config_dir: self.bundle.clone(),
                dev: dev_settings.clone(),
                explicit_path: explicit,
            },
        )?;
        if dev_settings.is_some() {
            println!("greentic-pack -> {} ✅", pack_command.display());
        }
        if demo_debug_enabled() {
            println!(
                "[demo] doctor bundle={} greentic-pack={}",
                self.bundle.display(),
                pack_command.display()
            );
        }
        demo::demo_doctor(&self.bundle, &pack_command)
    }
}

fn project_root(arg: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    Ok(arg.unwrap_or(env::current_dir()?))
}

fn resolve_state_dir(state_dir: Option<PathBuf>, bundle: Option<&PathBuf>) -> PathBuf {
    if let Some(state_dir) = state_dir {
        return state_dir;
    }
    if let Some(bundle) = bundle {
        return bundle.join("state");
    }
    PathBuf::from("state")
}

fn demo_debug_enabled() -> bool {
    matches!(
        std::env::var("GREENTIC_OPERATOR_DEMO_DEBUG").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn run_demo_up_setup(
    bundle: &Path,
    domains: &[Domain],
    setup_input: &Path,
    tenant_override: Option<String>,
    team_override: Option<String>,
    env: &str,
    runner_binary: Option<PathBuf>,
    public_base_url: Option<String>,
    secrets_manager: Option<DynSecretsManager>,
) -> anyhow::Result<()> {
    let providers_input = ProvidersInput::load(setup_input)?;
    for domain in domains {
        let provider_map = match providers_input.providers_for_domain(*domain) {
            Some(map) if !map.is_empty() => map,
            _ => {
                println!(
                    "[demo] no providers configured for domain {}; skipping provider setup",
                    domains::domain_name(*domain)
                );
                continue;
            }
        };
        let tenants = if let Some(tenant) = tenant_override.as_ref() {
            vec![tenant.clone()]
        } else {
            let discovered = discover_tenants(bundle, *domain)?;
            if discovered.is_empty() {
                println!(
                    "[demo] no tenants discovered for domain {}; skipping",
                    domains::domain_name(*domain)
                );
                continue;
            }
            discovered
        };
        let provider_keys: BTreeSet<String> = provider_map.keys().cloned().collect();
        let mut map = serde_json::Map::new();
        for (provider, value) in provider_map {
            map.insert(provider.clone(), value.clone());
        }
        let setup_answers =
            SetupInputAnswers::new(serde_json::Value::Object(map), provider_keys.clone())?;
        for tenant in tenants {
            run_domain_command(DomainRunArgs {
                root: bundle.to_path_buf(),
                state_root: None,
                domain: *domain,
                action: DomainAction::Setup,
                tenant,
                team: team_override.clone(),
                provider_filter: None,
                dry_run: false,
                format: PlanFormat::Text,
                parallel: 1,
                allow_missing_setup: true,
                online: false,
                secrets_env: Some(env.to_string()),
                secrets_bin: None,
                runner_binary: runner_binary.clone(),
                best_effort: false,
                discovered_providers: None,
                setup_input: None,
                allowed_providers: Some(provider_keys.clone()),
                preloaded_setup_answers: Some(setup_answers.clone()),
                public_base_url: public_base_url.clone(),
                secrets_manager: secrets_manager.clone(),
            })?;
        }
    }
    Ok(())
}

fn demo_provider_files(
    root: &Path,
    tenant: &str,
    team: Option<&str>,
    domain: Domain,
) -> anyhow::Result<Option<std::collections::BTreeSet<String>>> {
    let resolved = demo_resolved_manifest_path(root, tenant, team);
    if !resolved.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(resolved)?;
    let manifest: DemoResolvedManifest = serde_yaml_bw::from_str(&contents)?;
    let key = match domain {
        Domain::Messaging => "messaging",
        Domain::Events => "events",
        Domain::Secrets => "secrets",
    };
    let Some(list) = manifest.providers.get(key) else {
        return Ok(Some(std::collections::BTreeSet::new()));
    };
    let mut files = std::collections::BTreeSet::new();
    for path in list {
        if let Some(name) = Path::new(path).file_name().and_then(|value| value.to_str()) {
            files.insert(name.to_string());
        }
    }
    Ok(Some(files))
}

fn demo_resolved_manifest_path(root: &Path, tenant: &str, team: Option<&str>) -> PathBuf {
    root.join("resolved")
        .join(resolved_manifest_filename(tenant, team))
}

fn demo_state_resolved_manifest_path(root: &Path, tenant: &str, team: Option<&str>) -> PathBuf {
    root.join("state")
        .join("resolved")
        .join(resolved_manifest_filename(tenant, team))
}

fn resolved_manifest_filename(tenant: &str, team: Option<&str>) -> String {
    match team {
        Some(team) => format!("{tenant}.{team}.yaml"),
        None => format!("{tenant}.yaml"),
    }
}

fn demo_bundle_gmap_path(bundle: &Path, tenant: &str, team: Option<&str>) -> PathBuf {
    let mut path = bundle.join("tenants").join(tenant);
    if let Some(team) = team {
        path = path.join("teams").join(team).join("team.gmap");
    } else {
        path = path.join("tenant.gmap");
    }
    path
}

fn copy_resolved_manifest(bundle: &Path, tenant: &str, team: Option<&str>) -> anyhow::Result<()> {
    let src = demo_state_resolved_manifest_path(bundle, tenant, team);
    if !src.exists() {
        return Err(anyhow::anyhow!(
            "resolved manifest not found at {}",
            src.display()
        ));
    }
    let dst = demo_resolved_manifest_path(bundle, tenant, team);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dst)?;
    Ok(())
}

fn discovery_map(
    providers: &[discovery::DetectedProvider],
) -> std::collections::BTreeMap<PathBuf, discovery::DetectedProvider> {
    let mut map = std::collections::BTreeMap::new();
    for provider in providers {
        map.insert(provider.pack_path.clone(), provider.clone());
    }
    map
}

fn provider_filter_matches(pack: &domains::ProviderPack, filter: &str) -> bool {
    let file_stem = pack
        .file_name
        .strip_suffix(".gtpack")
        .unwrap_or(&pack.file_name);
    pack.pack_id == filter
        || pack.file_name == filter
        || file_stem == filter
        || pack.pack_id.contains(filter)
        || pack.file_name.contains(filter)
        || file_stem.contains(filter)
}

fn resolve_demo_provider_pack(
    root: &Path,
    tenant: &str,
    team: Option<&str>,
    provider: &str,
    domain: Domain,
) -> anyhow::Result<domains::ProviderPack> {
    let is_demo_bundle = root.join("greentic.demo.yaml").exists();
    let mut packs = if is_demo_bundle {
        domains::discover_provider_packs_cbor_only(root, domain)?
    } else {
        domains::discover_provider_packs(root, domain)?
    };
    if is_demo_bundle && let Some(allowed) = demo_provider_files(root, tenant, team, domain)? {
        packs.retain(|pack| allowed.contains(&pack.file_name));
    }
    packs.retain(|pack| provider_filter_matches(pack, provider));
    if packs.is_empty() {
        return Err(anyhow::anyhow!(
            "No provider packs matched. Try --provider <pack_id>."
        ));
    }
    packs.sort_by(|a, b| a.path.cmp(&b.path));
    if packs.len() > 1 {
        let names = packs
            .iter()
            .map(|pack| pack.file_name.clone())
            .collect::<Vec<_>>();
        return Err(anyhow::anyhow!(
            "Multiple provider packs matched: {}. Use a more specific --provider.",
            names.join(", ")
        ));
    }
    Ok(packs.remove(0))
}

fn ensure_requirements_flow(pack: &domains::ProviderPack) -> Result<(), String> {
    if pack.entry_flows.iter().any(|flow| flow == "requirements") {
        return Ok(());
    }
    Err(
        "requirements flow not found in provider pack; ask the provider pack to include an entry flow named 'requirements'."
            .to_string(),
    )
}

#[derive(serde::Deserialize)]
struct DemoResolvedManifest {
    #[serde(default)]
    providers: std::collections::BTreeMap<String, Vec<String>>,
}

fn resolve_dev_settings(
    settings: &settings::OperatorSettings,
    config: Option<&config::OperatorConfig>,
    overrides: &DevModeArgs,
    config_dir: &Path,
) -> anyhow::Result<Option<DevSettingsResolved>> {
    let global = settings.dev.to_dev_settings();
    let merged = merge_settings(config.and_then(|cfg| cfg.dev.clone()), Some(global));
    effective_dev_settings(overrides.to_overrides(), merged, config_dir)
}

impl DevModeArgs {
    fn to_overrides(&self) -> DevCliOverrides {
        DevCliOverrides {
            mode: self.dev_mode.map(Into::into),
            root: self.dev_root.clone(),
            profile: self.dev_profile.map(Into::into),
            target_dir: self.dev_target_dir.clone(),
        }
    }
}

impl From<DevModeArg> for DevMode {
    fn from(value: DevModeArg) -> Self {
        match value {
            DevModeArg::Auto => DevMode::Auto,
            DevModeArg::On => DevMode::On,
            DevModeArg::Off => DevMode::Off,
        }
    }
}

impl From<DevProfileArg> for DevProfile {
    fn from(value: DevProfileArg) -> Self {
        match value {
            DevProfileArg::Debug => DevProfile::Debug,
            DevProfileArg::Release => DevProfile::Release,
        }
    }
}

struct DomainRunArgs {
    root: PathBuf,
    state_root: Option<PathBuf>,
    domain: Domain,
    action: DomainAction,
    tenant: String,
    team: Option<String>,
    provider_filter: Option<String>,
    dry_run: bool,
    format: PlanFormat,
    parallel: usize,
    allow_missing_setup: bool,
    online: bool,
    secrets_env: Option<String>,
    secrets_bin: Option<PathBuf>,
    runner_binary: Option<PathBuf>,
    best_effort: bool,
    discovered_providers: Option<Vec<discovery::DetectedProvider>>,
    setup_input: Option<PathBuf>,
    allowed_providers: Option<BTreeSet<String>>,
    preloaded_setup_answers: Option<SetupInputAnswers>,
    public_base_url: Option<String>,
    secrets_manager: Option<DynSecretsManager>,
}

fn run_domain_command(args: DomainRunArgs) -> anyhow::Result<()> {
    let is_demo_bundle = args.root.join("greentic.demo.yaml").exists();
    let mut packs = if is_demo_bundle {
        domains::discover_provider_packs_cbor_only(&args.root, args.domain)?
    } else {
        domains::discover_provider_packs(&args.root, args.domain)?
    };
    let provider_map = args.discovered_providers.as_ref().map(|providers| {
        let mut map = std::collections::BTreeMap::new();
        for provider in providers {
            map.insert(provider.pack_path.clone(), provider.clone());
        }
        map
    });
    if let Some(provider_map) = provider_map.as_ref() {
        packs.retain(|pack| provider_map.contains_key(&pack.path));
        packs.sort_by(|a, b| a.path.cmp(&b.path));
    }
    if is_demo_bundle
        && let Some(allowed) =
            demo_provider_files(&args.root, &args.tenant, args.team.as_deref(), args.domain)?
    {
        packs.retain(|pack| allowed.contains(&pack.file_name));
    }
    if args.action == DomainAction::Setup {
        let setup_flow = domains::config(args.domain).setup_flow;
        let missing: Vec<String> = packs
            .iter()
            .filter(|pack| !pack.entry_flows.iter().any(|flow| flow == setup_flow))
            .map(|pack| pack.file_name.clone())
            .collect();
        if !missing.is_empty() && !args.allow_missing_setup {
            if args.best_effort {
                println!(
                    "Best-effort: skipped {} pack(s) missing {setup_flow}.",
                    missing.len()
                );
                packs.retain(|pack| pack.entry_flows.iter().any(|flow| flow == setup_flow));
            } else {
                return Err(anyhow::anyhow!(
                    "missing {setup_flow} in packs: {}",
                    missing.join(", ")
                ));
            }
        }
    }
    if packs.is_empty() {
        return Ok(());
    }
    if let Some(allowed) = args.allowed_providers.as_ref() {
        let missing = filter_packs_by_allowed(&mut packs, allowed);
        if !missing.is_empty() {
            println!(
                "[warn] skip setup domain={} missing packs: {}",
                domains::domain_name(args.domain),
                missing.join(", ")
            );
        }
    }
    let setup_answers = if let Some(preloaded) = args.preloaded_setup_answers.clone() {
        Some(preloaded)
    } else if let Some(path) = args.setup_input.as_ref() {
        let provider_keys: BTreeSet<String> =
            packs.iter().map(|pack| pack.pack_id.clone()).collect();
        Some(SetupInputAnswers::new(
            load_setup_input(path)?,
            provider_keys,
        )?)
    } else {
        None
    };
    let interactive = args.setup_input.is_none();
    let plan = domains::plan_runs(
        args.domain,
        args.action,
        &packs,
        args.provider_filter.as_deref(),
        args.allow_missing_setup,
    )?;

    if plan.is_empty() {
        if is_demo_bundle {
            println!("No provider packs matched. Try --provider <pack_id>.");
        } else {
            println!("No provider packs matched. Try --provider <pack_id> or --project-root.");
        }
        return Ok(());
    }

    if args.dry_run {
        render_plan(&plan, args.format)?;
        return Ok(());
    }

    let runner_binary = resolve_demo_runner_binary(&args.root, args.runner_binary)?;
    let dist_offline = !args.online;
    let state_root = args.state_root.as_ref().unwrap_or(&args.root);
    run_plan(
        &args.root,
        state_root,
        args.domain,
        args.action,
        &args.tenant,
        args.team.as_deref(),
        plan,
        args.parallel,
        dist_offline,
        args.secrets_env.as_deref(),
        args.secrets_bin.as_deref(),
        runner_binary,
        args.best_effort,
        provider_map,
        setup_answers,
        interactive,
        args.public_base_url.clone(),
        args.secrets_manager.clone(),
    )
}

fn filter_packs_by_allowed(
    packs: &mut Vec<domains::ProviderPack>,
    allowed: &BTreeSet<String>,
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    packs.retain(|pack| {
        if allowed.contains(&pack.pack_id) {
            seen.insert(pack.pack_id.clone());
            true
        } else {
            false
        }
    });
    allowed
        .iter()
        .filter(|value| !seen.contains(*value))
        .cloned()
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn run_plan(
    root: &Path,
    state_root: &Path,
    domain: Domain,
    action: DomainAction,
    tenant: &str,
    team: Option<&str>,
    plan: Vec<domains::PlannedRun>,
    parallel: usize,
    dist_offline: bool,
    secrets_env: Option<&str>,
    secrets_bin: Option<&Path>,
    runner_binary: Option<PathBuf>,
    best_effort: bool,
    provider_map: Option<std::collections::BTreeMap<PathBuf, discovery::DetectedProvider>>,
    setup_answers: Option<SetupInputAnswers>,
    interactive: bool,
    public_base_url: Option<String>,
    secrets_manager: Option<DynSecretsManager>,
) -> anyhow::Result<()> {
    let setup_answers = setup_answers.map(Arc::new);
    let plan_public_base_url = public_base_url.map(Arc::new);
    let plan_secrets_manager = secrets_manager;
    if parallel <= 1 {
        let mut errors = Vec::new();
        for item in plan {
            let result = run_plan_item(
                root,
                state_root,
                domain,
                action,
                tenant,
                team,
                &item,
                dist_offline,
                secrets_env,
                secrets_bin,
                runner_binary.as_deref(),
                setup_answers.as_deref(),
                provider_map.as_ref(),
                interactive,
                plan_public_base_url.clone(),
                plan_secrets_manager.clone(),
            );
            if let Err(err) = result {
                if best_effort {
                    errors.push(err);
                } else {
                    return Err(err);
                }
            }
        }
        if best_effort && !errors.is_empty() {
            println!("Best-effort: {} flow(s) failed.", errors.len());
            return Ok(());
        }
        return Ok(());
    }

    let mut handles = Vec::new();
    let plan = std::sync::Arc::new(std::sync::Mutex::new(plan));
    let errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

    for _ in 0..parallel {
        let plan = plan.clone();
        let errors = errors.clone();
        let root = root.to_path_buf();
        let state_root = state_root.to_path_buf();
        let tenant = tenant.to_string();
        let team = team.map(|value| value.to_string());
        let secrets_env = secrets_env.map(|value| value.to_string());
        let secrets_bin = secrets_bin.map(|value| value.to_path_buf());
        let runner_binary = runner_binary.clone();
        let provider_map = provider_map.clone();
        let setup_answers = setup_answers.clone();
        let interactive_flag = interactive;
        let thread_public_base_url = plan_public_base_url.clone();
        let thread_secrets_manager = plan_secrets_manager.clone();
        handles.push(std::thread::spawn(move || {
            loop {
                let next = {
                    let mut queue = plan.lock().unwrap();
                    queue.pop()
                };
                let Some(item) = next else {
                    break;
                };
                let result = run_plan_item(
                    &root,
                    &state_root,
                    domain,
                    action,
                    &tenant,
                    team.as_deref(),
                    &item,
                    dist_offline,
                    secrets_env.as_deref(),
                    secrets_bin.as_deref(),
                    runner_binary.as_deref(),
                    setup_answers.as_deref(),
                    provider_map.as_ref(),
                    interactive_flag,
                    thread_public_base_url.clone(),
                    thread_secrets_manager.clone(),
                );
                if let Err(err) = result {
                    errors.lock().unwrap().push(err);
                }
            }
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }

    let errors = errors.lock().unwrap();
    if !errors.is_empty() {
        if best_effort {
            println!("Best-effort: {} flow(s) failed.", errors.len());
            return Ok(());
        }
        return Err(anyhow::anyhow!("{} flow(s) failed.", errors.len()));
    }
    Ok(())
}

fn render_plan(plan: &[domains::PlannedRun], format: PlanFormat) -> anyhow::Result<()> {
    match format {
        PlanFormat::Text => {
            println!("Plan:");
            for item in plan {
                println!("  {} -> {}", item.pack.file_name, item.flow_id);
            }
            Ok(())
        }
        PlanFormat::Json => {
            let json = serde_json::to_string_pretty(plan)?;
            println!("{json}");
            Ok(())
        }
        PlanFormat::Yaml => {
            let yaml = serde_yaml_bw::to_string(plan)?;
            print!("{yaml}");
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_plan_item(
    _root: &Path,
    state_root: &Path,
    domain: Domain,
    action: DomainAction,
    tenant: &str,
    team: Option<&str>,
    item: &domains::PlannedRun,
    dist_offline: bool,
    secrets_env: Option<&str>,
    secrets_bin: Option<&Path>,
    runner_binary: Option<&Path>,
    setup_answers: Option<&SetupInputAnswers>,
    provider_map: Option<&std::collections::BTreeMap<PathBuf, discovery::DetectedProvider>>,
    interactive: bool,
    public_base_url: Option<Arc<String>>,
    secrets_manager: Option<DynSecretsManager>,
) -> anyhow::Result<()> {
    let provider_id = provider_id_for_pack(&item.pack.path, &item.pack.pack_id, provider_map);
    let env_value = secrets_env
        .map(|value| value.to_string())
        .unwrap_or_else(crate::tools::secrets::resolve_env);

    if domain == Domain::Messaging && action == DomainAction::Setup {
        if let Some(manager) = secrets_manager.as_ref() {
            match secrets_gate::check_provider_secrets(
                manager,
                &env_value,
                tenant,
                team,
                &item.pack.path,
                &provider_id,
            ) {
                Ok(Some(missing)) => {
                    let formatted = missing
                        .iter()
                        .map(|entry| format!("  - {entry}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    println!(
                        "[warn] skip setup domain={} tenant={} provider={}: missing secrets:\n{formatted}",
                        domains::domain_name(domain),
                        tenant,
                        provider_id
                    );
                    return Ok(());
                }
                Ok(None) => {}
                Err(err) => {
                    println!(
                        "[warn] skip setup domain={} tenant={} provider={}: secrets check failed: {err}",
                        domains::domain_name(domain),
                        tenant,
                        provider_id
                    );
                    return Ok(());
                }
            }
        }
    }

    let setup_values = if action == DomainAction::Setup {
        Some(collect_setup_answers(
            &item.pack.path,
            &item.pack.pack_id,
            setup_answers,
            interactive,
        )?)
    } else {
        None
    };

    let public_base_url_ref = public_base_url.as_deref().map(|value| value.as_str());
    let input = build_input_payload(
        state_root,
        domain,
        tenant,
        team,
        Some(&item.pack.pack_id),
        setup_values.as_ref(),
        public_base_url_ref,
        &env_value,
    );
    if demo_debug_enabled() {
        println!(
            "[demo] setup input pack={} flow={} input={}",
            item.pack.file_name,
            item.flow_id,
            serde_json::to_string(&input).unwrap_or_else(|_| "<invalid-json>".to_string())
        );
    }
    if let Some(runner_binary) = runner_binary {
        let run_dir = state_layout::run_dir(state_root, domain, &item.pack.pack_id, &item.flow_id)?;
        std::fs::create_dir_all(&run_dir)?;
        let input_path = run_dir.join("input.json");
        let input_json = serde_json::to_string_pretty(&input)?;
        std::fs::write(&input_path, input_json)?;

        let runner_flavor = runner_integration::detect_runner_flavor(runner_binary);
        let output = runner_integration::run_flow_with_options(
            runner_binary,
            &item.pack.path,
            &item.flow_id,
            &input,
            runner_integration::RunFlowOptions {
                dist_offline,
                tenant: Some(tenant),
                team,
                artifacts_dir: Some(&run_dir),
                runner_flavor,
            },
        )?;
        write_runner_cli_artifacts(&run_dir, &output)?;
        if action == DomainAction::Setup {
            let providers_root = state_root
                .join("state")
                .join("runtime")
                .join(tenant)
                .join("providers");
            let setup_path = providers_root.join(format!("{provider_id}.setup.json"));
            crate::providers::write_run_output(&setup_path, &provider_id, &item.flow_id, &output)?;
        }
        let exit = format_runner_exit(&output);
        if output.status.success() {
            println!("{} {} -> {}", item.pack.file_name, item.flow_id, exit);
        } else if let Some(summary) = summarize_runner_error(&output) {
            println!(
                "{} {} -> {} ({})",
                item.pack.file_name, item.flow_id, exit, summary
            );
        } else {
            println!("{} {} -> {}", item.pack.file_name, item.flow_id, exit);
        }
    } else {
        let output = runner_exec::run_provider_pack_flow(runner_exec::RunRequest {
            root: state_root.to_path_buf(),
            domain,
            pack_path: item.pack.path.clone(),
            pack_label: item.pack.pack_id.clone(),
            flow_id: item.flow_id.clone(),
            tenant: tenant.to_string(),
            team: team.map(|value| value.to_string()),
            input,
            dist_offline,
        })
        .map_err(|err| {
            let message = err.to_string();
            if message.contains("manifest.cbor is invalid") {
                if let Ok(Some(detail)) = domains::manifest_cbor_issue_detail(&item.pack.path) {
                    return anyhow::anyhow!(
                        "pack verification failed for {}: {}",
                        item.pack.path.display(),
                        detail
                    );
                }
                return anyhow::anyhow!(
                    "pack verification failed for {}: {message}",
                    item.pack.path.display()
                );
            }
            err
        })?;
        if action == DomainAction::Setup {
            let providers_root = state_root
                .join("state")
                .join("runtime")
                .join(tenant)
                .join("providers");
            let setup_path = providers_root.join(format!("{provider_id}.setup.json"));
            crate::providers::write_run_result(
                &setup_path,
                &provider_id,
                &item.flow_id,
                &output.result,
            )?;
        }
        println!(
            "{} {} -> {:?}",
            item.pack.file_name, item.flow_id, output.result.status
        );
    }

    if domain == Domain::Messaging && action == DomainAction::Setup {
        let setup_flow = domains::config(domain).setup_flow;
        if item.flow_id == setup_flow {
            let Some(secrets_bin) = secrets_bin else {
                return Ok(());
            };
            let status = crate::tools::secrets::run_init(
                state_root,
                Some(secrets_bin),
                &env_value,
                tenant,
                team,
                &item.pack.path,
                true,
            )?;
            if !status.success() {
                let code = status.code().unwrap_or(1);
                return Err(anyhow::anyhow!(
                    "greentic-secrets init failed with exit code {code}"
                ));
            }
        }
    }
    Ok(())
}

fn resolve_demo_runner_binary(
    config_dir: &Path,
    runner_binary: Option<PathBuf>,
) -> anyhow::Result<Option<PathBuf>> {
    let Some(runner_binary) = runner_binary else {
        return Ok(None);
    };
    let runner_str = runner_binary.to_string_lossy();
    let (name, explicit) = if looks_like_path_str(&runner_str) {
        let name = runner_binary
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("greentic-runner")
            .to_string();
        (name, Some(runner_binary))
    } else {
        (runner_str.to_string(), None)
    };
    let resolved = bin_resolver::resolve_binary(
        &name,
        &ResolveCtx {
            config_dir: config_dir.to_path_buf(),
            dev: None,
            explicit_path: explicit,
        },
    )?;
    Ok(Some(resolved))
}

fn write_runner_cli_artifacts(
    run_dir: &Path,
    output: &runner_integration::RunnerOutput,
) -> anyhow::Result<()> {
    let run_json = run_dir.join("run.json");
    let summary_path = run_dir.join("summary.txt");
    let stdout_path = run_dir.join("stdout.txt");
    let stderr_path = run_dir.join("stderr.txt");

    let json = serde_json::json!({
        "status": {
            "success": output.status.success(),
            "code": output.status.code(),
        },
        "stdout": output.stdout,
        "stderr": output.stderr,
        "parsed": output.parsed,
    });
    let json = serde_json::to_string_pretty(&json)?;
    std::fs::write(run_json, json)?;
    std::fs::write(stdout_path, &output.stdout)?;
    std::fs::write(stderr_path, &output.stderr)?;

    let summary = format!(
        "success: {}\nexit_code: {}\n",
        output.status.success(),
        output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".to_string())
    );
    std::fs::write(summary_path, summary)?;
    Ok(())
}

fn format_runner_exit(output: &runner_integration::RunnerOutput) -> String {
    if let Some(code) = output.status.code() {
        return format!("exit={code}");
    }
    if output.status.success() {
        return "exit=0".to_string();
    }
    "exit=signal".to_string()
}

fn summarize_runner_error(output: &runner_integration::RunnerOutput) -> Option<String> {
    output
        .stderr
        .lines()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .map(|line| line.to_string())
}

fn build_domain_env(
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

fn event_component_ids(services: &config::OperatorServicesConfig) -> Vec<String> {
    event_components(services)
        .into_iter()
        .map(|component| component.id)
        .collect()
}

fn resolve_event_components(
    root: &Path,
    config: Option<&config::OperatorConfig>,
    services: &config::OperatorServicesConfig,
    dev_settings: &Option<DevSettingsResolved>,
) -> anyhow::Result<Vec<crate::services::ComponentSpec>> {
    let mut specs = Vec::new();
    for component in event_components(services) {
        let explicit = if looks_like_path_str(&component.binary) {
            Some(PathBuf::from(&component.binary))
        } else {
            config::binary_override(config, &component.binary, root)
        };
        let resolved = bin_resolver::resolve_binary(
            &component.binary,
            &ResolveCtx {
                config_dir: root.to_path_buf(),
                dev: dev_settings.clone(),
                explicit_path: explicit,
            },
        )?;
        specs.push(crate::services::ComponentSpec {
            id: component.id,
            binary: resolved.to_string_lossy().to_string(),
            args: component.args,
        });
    }
    Ok(specs)
}

fn event_components(
    services: &config::OperatorServicesConfig,
) -> Vec<config::ServiceComponentConfig> {
    if services.events.components.is_empty() {
        return config::default_events_components();
    }
    services.events.components.clone()
}

fn provider_id_for_pack(
    pack_path: &Path,
    fallback: &str,
    provider_map: Option<&std::collections::BTreeMap<PathBuf, discovery::DetectedProvider>>,
) -> String {
    provider_map
        .and_then(|map| map.get(pack_path))
        .map(|provider| provider.provider_id.clone())
        .unwrap_or_else(|| fallback.to_string())
}

fn looks_like_path_str(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || Path::new(value).is_absolute()
}

fn build_input_payload(
    root: &Path,
    domain: Domain,
    tenant: &str,
    team: Option<&str>,
    pack_id: Option<&str>,
    setup_answers: Option<&serde_json::Value>,
    public_base_url: Option<&str>,
    env: &str,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "tenant": tenant,
    });
    if let Some(team) = team {
        payload["team"] = serde_json::Value::String(team.to_string());
    }

    let resolved_public_base_url = public_base_url.map(|value| value.to_string()).or_else(|| {
        if matches!(domain, Domain::Messaging | Domain::Events) {
            read_public_base_url(root, tenant, team)
        } else {
            None
        }
    });

    if matches!(domain, Domain::Messaging | Domain::Events) {
        let mut config = serde_json::json!({});
        if let Some(url) = resolved_public_base_url.as_ref() {
            payload["public_base_url"] = serde_json::Value::String(url.clone());
            config["public_base_url"] = serde_json::Value::String(url.clone());
        }
        payload["config"] = config;
    }

    if let Some(pack_id) = pack_id
        && let Some(config_map) = payload
            .get_mut("config")
            .and_then(|value| value.as_object_mut())
    {
        config_map.insert(
            "id".to_string(),
            serde_json::Value::String(pack_id.to_string()),
        );
    }
    if let Some(pack_id) = pack_id {
        payload["id"] = serde_json::Value::String(pack_id.to_string());
    }
    if let Some(answers) = setup_answers {
        payload["setup_answers"] = answers.clone();
        if let Ok(json) = serde_json::to_string(answers) {
            payload["answers_json"] = serde_json::Value::String(json);
        }
    }
    let mut tenant_ctx = serde_json::json!({
        "env": env,
        "tenant": tenant,
        "tenant_id": tenant,
    });
    if let Some(team) = team {
        tenant_ctx["team"] = serde_json::Value::String(team.to_string());
        tenant_ctx["team_id"] = serde_json::Value::String(team.to_string());
    }
    let msg_id = pack_id
        .map(|value| format!("{value}.setup"))
        .unwrap_or_else(|| "setup".to_string());
    let mut metadata = serde_json::json!({});
    if let Some(url) = resolved_public_base_url {
        metadata["public_base_url"] = serde_json::Value::String(url);
    }
    let msg = serde_json::json!({
        "id": msg_id,
        "tenant": tenant_ctx,
        "channel": "setup",
        "session_id": "setup",
        "metadata": metadata,
    });
    payload["msg"] = msg;
    payload["payload"] = serde_json::json!({});
    payload
}

fn read_public_base_url(root: &Path, tenant: &str, team: Option<&str>) -> Option<String> {
    let team_id = team.unwrap_or("default");
    let paths = crate::runtime_state::RuntimePaths::new(root.join("state"), tenant, team_id);
    let path = crate::cloudflared::public_url_path(&paths);
    let contents = std::fs::read_to_string(path).ok()?;
    crate::cloudflared::parse_public_url(&contents)
}

fn parse_kv(input: &str) -> anyhow::Result<(String, JsonValue)> {
    let mut parts = input.splitn(2, '=');
    let key = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("expected key=value"))?;
    let value = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected key=value"))?
        .trim();
    if value.eq_ignore_ascii_case("true") {
        return Ok((key.to_string(), JsonValue::Bool(true)));
    }
    if value.eq_ignore_ascii_case("false") {
        return Ok((key.to_string(), JsonValue::Bool(false)));
    }
    if let Ok(int_value) = value.parse::<i64>() {
        return Ok((key.to_string(), JsonValue::Number(int_value.into())));
    }
    Ok((key.to_string(), JsonValue::String(value.to_string())))
}

fn merge_args(
    args_json: Option<&str>,
    args: &[String],
) -> anyhow::Result<JsonMap<String, JsonValue>> {
    let mut merged = JsonMap::new();
    if let Some(raw) = args_json {
        let parsed: JsonValue = serde_json::from_str(raw)?;
        let JsonValue::Object(map) = parsed else {
            return Err(anyhow::anyhow!("--args-json must be a JSON object"));
        };
        merged.extend(map);
    }
    for item in args {
        let (key, value) = parse_kv(item)?;
        merged.insert(key, value);
    }
    Ok(merged)
}

fn format_requirements_output(value: &JsonValue) -> Option<String> {
    let JsonValue::Object(map) = value else {
        return None;
    };
    let has_keys = map.contains_key("required_args")
        || map.contains_key("optional_args")
        || map.contains_key("examples")
        || map.contains_key("notes");
    if !has_keys {
        return None;
    }
    let mut output = String::new();
    if let Some(required) = map.get("required_args").and_then(JsonValue::as_array) {
        output.push_str("Required args:\n");
        for item in required {
            output.push_str("  - ");
            output.push_str(&format_requirements_item(item));
            output.push('\n');
        }
    }
    if let Some(optional) = map.get("optional_args").and_then(JsonValue::as_array) {
        output.push_str("Optional args:\n");
        for item in optional {
            output.push_str("  - ");
            output.push_str(&format_requirements_item(item));
            output.push('\n');
        }
    }
    if let Some(examples) = map.get("examples").and_then(JsonValue::as_array) {
        output.push_str("Examples:\n");
        for item in examples {
            let pretty = serde_json::to_string_pretty(item).unwrap_or_else(|_| item.to_string());
            if pretty.contains('\n') {
                output.push_str("  -\n");
                for line in pretty.lines() {
                    output.push_str("    ");
                    output.push_str(line);
                    output.push('\n');
                }
            } else {
                output.push_str("  - ");
                output.push_str(&pretty);
                output.push('\n');
            }
        }
    }
    if let Some(notes) = map.get("notes").and_then(JsonValue::as_str) {
        output.push_str("Notes:\n");
        output.push_str(notes);
        output.push('\n');
    }
    Some(output.trim_end().to_string())
}

fn format_requirements_item(value: &JsonValue) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

struct FlowOutcome {
    success: bool,
    output: Option<JsonValue>,
    raw: Option<String>,
    error: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn run_demo_flow(
    root: &Path,
    domain: Domain,
    tenant: &str,
    team: Option<&str>,
    pack: &domains::ProviderPack,
    flow_id: &str,
    input: &JsonValue,
    runner_binary: Option<&Path>,
) -> anyhow::Result<FlowOutcome> {
    let dist_offline = true;
    if let Some(runner_binary) = runner_binary {
        let run_dir = state_layout::run_dir(root, domain, &pack.pack_id, flow_id)?;
        std::fs::create_dir_all(&run_dir)?;
        let input_path = run_dir.join("input.json");
        let input_json = serde_json::to_string_pretty(input)?;
        std::fs::write(&input_path, input_json)?;

        let runner_flavor = runner_integration::detect_runner_flavor(runner_binary);
        let output = runner_integration::run_flow_with_options(
            runner_binary,
            &pack.path,
            flow_id,
            input,
            runner_integration::RunFlowOptions {
                dist_offline,
                tenant: Some(tenant),
                team,
                artifacts_dir: Some(&run_dir),
                runner_flavor,
            },
        )?;
        write_runner_cli_artifacts(&run_dir, &output)?;
        let mut parsed = output.parsed.clone();
        if parsed.is_none() {
            parsed = serde_json::from_str(&output.stdout).ok();
        }
        if parsed.is_none() {
            parsed = read_transcript_outputs(&run_dir)?;
        }
        let error = summarize_runner_error(&output);
        let raw = if output.stdout.trim().is_empty() {
            None
        } else {
            Some(output.stdout.clone())
        };
        return Ok(FlowOutcome {
            success: output.status.success(),
            output: parsed,
            raw,
            error,
        });
    }

    let output = runner_exec::run_provider_pack_flow(runner_exec::RunRequest {
        root: root.to_path_buf(),
        domain,
        pack_path: pack.path.clone(),
        pack_label: pack.pack_id.clone(),
        flow_id: flow_id.to_string(),
        tenant: tenant.to_string(),
        team: team.map(|value| value.to_string()),
        input: input.clone(),
        dist_offline,
    })?;
    let parsed = read_transcript_outputs(&output.run_dir)?;
    Ok(FlowOutcome {
        success: output.result.status == RunStatus::Success,
        output: parsed,
        raw: None,
        error: output.result.error.clone(),
    })
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

impl From<DomainArg> for Domain {
    fn from(value: DomainArg) -> Self {
        match value {
            DomainArg::Messaging => Domain::Messaging,
            DomainArg::Events => Domain::Events,
            DomainArg::Secrets => Domain::Secrets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, path::PathBuf};

    #[test]
    fn parse_kv_infers_basic_types() {
        let (key, value) = parse_kv("a=1").unwrap();
        assert_eq!(key, "a");
        assert_eq!(value, JsonValue::Number(1.into()));

        let (key, value) = parse_kv("b=true").unwrap();
        assert_eq!(key, "b");
        assert_eq!(value, JsonValue::Bool(true));

        let (key, value) = parse_kv("c=hello").unwrap();
        assert_eq!(key, "c");
        assert_eq!(value, JsonValue::String("hello".to_string()));
    }

    #[test]
    fn merge_args_overrides_json() {
        let merged = merge_args(
            Some(r#"{"chat_id":1,"mode":"x"}"#),
            &["chat_id=2".to_string()],
        )
        .unwrap();
        assert_eq!(merged.get("chat_id"), Some(&JsonValue::Number(2.into())));
        assert_eq!(
            merged.get("mode"),
            Some(&JsonValue::String("x".to_string()))
        );
    }

    #[test]
    fn requirements_formatting_structured() {
        let value = serde_json::json!({
            "required_args": ["chat_id"],
            "optional_args": ["thread_id"],
            "examples": [{"chat_id": 1}],
            "notes": "Example note"
        });
        let rendered = format_requirements_output(&value).unwrap();
        assert!(rendered.contains("Required args:"));
        assert!(rendered.contains("Optional args:"));
        assert!(rendered.contains("Examples:"));
        assert!(rendered.contains("Notes:"));
    }

    #[test]
    fn requirements_missing_message() {
        let pack = domains::ProviderPack {
            pack_id: "demo".to_string(),
            file_name: "demo.gtpack".to_string(),
            path: PathBuf::from("demo.gtpack"),
            entry_flows: vec!["setup_default".to_string()],
        };
        let error = ensure_requirements_flow(&pack).unwrap_err();
        assert!(error.contains("requirements flow not found"));
    }

    #[test]
    fn filter_allowed_providers_moves_missing() {
        let mut packs = vec![
            domains::ProviderPack {
                pack_id: "messaging-telegram".to_string(),
                file_name: "telegram.gtpack".to_string(),
                path: PathBuf::from("telegram.gtpack"),
                entry_flows: vec!["setup_default".to_string()],
            },
            domains::ProviderPack {
                pack_id: "messaging-slack".to_string(),
                file_name: "slack.gtpack".to_string(),
                path: PathBuf::from("slack.gtpack"),
                entry_flows: vec!["setup_default".to_string()],
            },
        ];
        let allowed = vec![
            "messaging-telegram".to_string(),
            "messaging-email".to_string(),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        let missing = filter_packs_by_allowed(&mut packs, &allowed);
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].pack_id, "messaging-telegram");
        assert_eq!(missing, vec!["messaging-email".to_string()]);
    }
}
