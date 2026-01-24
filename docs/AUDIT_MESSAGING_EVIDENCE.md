# Messaging Audit Evidence (raw outputs)

## cargo tree (greentic-messaging)
```bash
cargo tree -i greentic-messaging || true
```
```text
error: package ID specification `greentic-messaging` did not match any packages
```

## cargo tree (greentic-runner-desktop)
```bash
cargo tree -i greentic-runner-desktop
```
```text
greentic-runner-desktop v0.4.42
└── greentic-operator v0.4.1 (/projects/ai/greentic-ng/greentic-operator)
```

## rg: messaging binary + messaging paths
```bash
rg -n "greentic-messaging|providers/messaging|messaging" src README.md
```
```text
README.md:13:# drop provider packs into providers/messaging/
README.md:49:- messaging: `providers/messaging/*.gtpack`
README.md:56:  messaging:
src/project/layout.rs:9:  messaging_command: greentic-messaging
src/project/layout.rs:22:    ensure_dir(&root.join("providers").join("messaging"))?;
src/state_layout.rs:34:        Domain::Messaging => "messaging",
src/discovery.rs:17:    pub messaging: bool
src/discovery.rs:74:        messaging: providers
src/discovery.rs:76:            .any(|provider| provider.domain == "messaging"),
src/config.rs:23:    pub messaging_command: Option<String>
src/config.rs:53:    pub messaging: DomainServicesConfig
src/config.rs:96:pub fn messaging_binary(config: Option<&OperatorConfig>, config_dir: &Path) -> BinaryConfig {
src/config.rs:98:        if let Some(path) = config_binary_path(config, "greentic-messaging", config_dir) {
src/config.rs:100:                name: "greentic-messaging".to_string(),
src/config.rs:108:            .and_then(|runtime| runtime.messaging_command.as_ref())
src/config.rs:112:                    name: "greentic-messaging".to_string(),
src/config.rs:124:        name: "greentic-messaging".to_string(),
src/doctor.rs:219:        Domain::Messaging => "messaging",
src/demo/runtime.rs:19:    messaging_command: Option<&str>,
src/demo/runtime.rs:40:    if let Some(messaging_command) = messaging_command {
src/demo/runtime.rs:42:        let name = services::messaging_name(tenant, team);
src/demo/runtime.rs:43:        let state = services::start_messaging_from_manifest(
src/demo/runtime.rs:48:            messaging_command,
src/demo/runtime.rs:50:        println!("messaging: {:?}", state);
src/demo/runtime.rs:52:        println!("messaging: skipped (disabled or no providers)");
src/demo/runtime.rs:248:    let state = services::stop_messaging(bundle_root, tenant, team)?;
src/demo/runtime.rs:249:    println!("messaging: {:?}", state);
src/demo/runtime.rs:274:    let messaging = services::messaging_status(bundle_root, tenant, team)?;
src/demo/runtime.rs:275:    println!("messaging: {:?}", messaging);
src/demo/runtime.rs:344:            let name = services::messaging_name(tenant, team);
src/demo/runtime.rs:350:                let fallback = services::messaging_name(tenant, None);
src/domains/mod.rs:44:            providers_dir: "providers/messaging",
src/domains/mod.rs:66:        Domain::Messaging => "validators-messaging.gtpack",
src/domains/mod.rs:238:        Domain::Messaging => "messaging",
src/services/mod.rs:2:mod messaging;
src/services/mod.rs:7:pub use messaging::{
src/services/mod.rs:8:    messaging_name, messaging_status, start_messaging, start_messaging_from_manifest,
src/services/mod.rs:9:    start_messaging_with_command, stop_messaging, tail_messaging_logs,
src/services/messaging.rs:17:pub fn start_messaging(
src/services/messaging.rs:23:    start_messaging_with_command(root, tenant, team, nats_url, "greentic-messaging")
src/services/messaging.rs:26:pub fn start_messaging_with_command(
src/services/messaging.rs:34:    let name = messaging_name(tenant, team);
src/services/messaging.rs:35:    start_messaging_from_manifest(root, &manifest_path, &name, nats_url, command)
src/services/messaging.rs:38:pub fn start_messaging_from_manifest(
src/services/messaging.rs:69:    for pack in messaging_adapter_packs(root, &manifest) {
src/services/messaging.rs:82:    let cwd = messaging_cwd(command, root);
src/services/messaging.rs:86:pub fn stop_messaging(
src/services/messaging.rs:91:    let name = messaging_name(tenant, team);
src/services/messaging.rs:96:pub fn messaging_status(
src/services/messaging.rs:101:    let name = messaging_name(tenant, team);
src/services/messaging.rs:106:pub fn tail_messaging_logs(root: &Path, tenant: &str, team: Option<&str>) -> anyhow::Result<()> {
src/services/messaging.rs:107:    let name = messaging_name(tenant, team);
src/services/messaging.rs:112:pub fn messaging_name(tenant: &str, team: Option<&str>) -> String {
src/services/messaging.rs:114:        Some(team) => format!("messaging-{tenant}-{team}"),
src/services/messaging.rs:115:        None => format!("messaging-{tenant}"),
src/services/messaging.rs:133:fn messaging_adapter_packs(root: &Path, manifest: &ResolvedManifest) -> Vec<String> {
src/services/messaging.rs:137:        && let Some(adapter_packs) = providers.get("messaging")
src/services/messaging.rs:179:fn messaging_cwd(command: &str, fallback: &Path) -> Option<std::path::PathBuf> {
src/cli.rs:232:    about = "Start local messaging services (and NATS unless disabled).",
src/cli.rs:233:    long_about = "Uses state/resolved/<tenant>[.<team>].yaml and launches greentic-messaging and optional NATS.",
src/cli.rs:253:    about = "Stop local messaging services (and NATS unless disabled).",
src/cli.rs:270:    about = "Show running status for local messaging services.",
src/cli.rs:271:    long_about = "Checks pidfiles for messaging and optional NATS.",
src/cli.rs:287:    about = "Tail logs for messaging or NATS.",
src/cli.rs:289:    after_help = "Main options:\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --service <messaging|nats>\n  --project-root <PATH> (default: current directory)"
src/cli.rs:305:    long_about = "Executes setup_default across providers and can auto-run secrets init for messaging.",
src/cli.rs:532:    after_help = "Main options:\n  --bundle <DIR>\n  --tenant <TENANT>\n\nOptional options:\n  --team <TEAM>\n  --domain <messaging|events|secrets|all> (default: all)\n  --provider <FILTER>\n  --dry-run\n  --format <text|json|yaml> (default: text)\n  --parallel <N> (default: 1)\n  --allow-missing-setup\n  --online\n  --secrets-env <ENV>\n  --secrets-bin <PATH>\n  --skip-secrets-init\n  --runner-binary <PATH>\n  --best-effort"
src/cli.rs:616:    after_help = "Main options:\n  <SERVICE> (messaging|nats|cloudflared)\n\nOptional options:\n  --tail\n  --tenant <TENANT> (default: demo)\n  --team <TEAM> (default: default)\n  --state-dir <PATH> (default: ./state or <bundle>/state)\n  --bundle <DIR> (legacy mode if --state-dir omitted)\n  --verbose\n  --no-nats"
src/cli.rs:619:    #[arg(default_value = "messaging")]
src/cli.rs:896:        let messaging_enabled = services
src/cli.rs:897:            .messaging
src/cli.rs:899:            .is_enabled(discovery.domains.messaging);
src/cli.rs:903:        if !self.no_nats && nats_url.is_none() && (messaging_enabled || events_enabled) {
src/cli.rs:911:        if messaging_enabled {
src/cli.rs:912:            let messaging_binary = config::messaging_binary(config.as_ref(), &root);
src/cli.rs:914:                &messaging_binary.name,
src/cli.rs:918:                    explicit_path: messaging_binary.explicit_path,
src/cli.rs:921:            let state = crate::services::start_messaging_with_command(
src/cli.rs:928:            println!("messaging: {:?}", state);
src/cli.rs:930:            println!("messaging: skipped (disabled or no providers)");
src/cli.rs:955:        let state = crate::services::stop_messaging(&root, &self.tenant, self.team.as_deref())?;
src/cli.rs:956:        println!("messaging: {:?}", state);
src/cli.rs:978:            "detected domains: messaging={} events={}",
src/cli.rs:979:            discovery.domains.messaging, discovery.domains.events
src/cli.rs:991:        let messaging_enabled = services
src/cli.rs:992:            .messaging
src/cli.rs:994:            .is_enabled(discovery.domains.messaging);
src/cli.rs:997:        if messaging_enabled {
src/cli.rs:998:            let messaging =
src/cli.rs:999:                crate::services::messaging_status(&root, &self.tenant, self.team.as_deref())?;
src/cli.rs:1000:            println!("messaging: {:?}", messaging);
src/cli.rs:1002:            println!("messaging: skipped (disabled or no providers)");
src/cli.rs:1027:                crate::services::tail_messaging_logs(&root, &self.tenant, self.team.as_deref())
src/cli.rs:1211:            let messaging_enabled = services
src/cli.rs:1212:                .messaging
src/cli.rs:1214:                .is_enabled(discovery.domains.messaging);
src/cli.rs:1216:            let messaging_command = if messaging_enabled {
src/cli.rs:1217:                let messaging_binary = config::messaging_binary(config.as_ref(), &bundle);
src/cli.rs:1219:                    &messaging_binary.name,
src/cli.rs:1223:                        explicit_path: messaging_binary.explicit_path,
src/cli.rs:1262:                messaging_command.as_deref(),
src/cli.rs:1334:                if discovery.domains.messaging {
src/cli.rs:1604:        Domain::Messaging => "messaging",
```

## rg: flow names used by operator
```bash
rg -n "setup_default|verify_webhooks|diagnostics" src/domains/mod.rs src/providers.rs src/cli.rs
```
```text
src/domains/mod.rs:23:    pub diagnostics_flow: &'static str,
src/domains/mod.rs:45:            setup_flow: "setup_default",
src/domains/mod.rs:46:            diagnostics_flow: "diagnostics",
src/domains/mod.rs:47:            verify_flows: &["verify_webhooks"],
src/domains/mod.rs:51:            setup_flow: "setup_default",
src/domains/mod.rs:52:            diagnostics_flow: "diagnostics",
src/domains/mod.rs:57:            setup_flow: "setup_default",
src/domains/mod.rs:58:            diagnostics_flow: "diagnostics",
src/domains/mod.rs:116:        DomainAction::Diagnostics => vec![cfg.diagnostics_flow],
src/providers.rs:16:    pub verify_webhooks: bool,
src/providers.rs:80:            .unwrap_or_else(|| "setup_default".to_string());
src/providers.rs:90:        if options.verify_webhooks {
src/providers.rs:94:                .unwrap_or_else(|| "verify_webhooks".to_string());
src/cli.rs:305:    long_about = "Executes setup_default across providers and can auto-run secrets init for messaging.",
src/cli.rs:336:    about = "Run provider diagnostics flows for a domain.",
src/cli.rs:337:    long_about = "Executes diagnostics for each provider pack that defines it.",
src/cli.rs:509:    verify_webhooks: bool,
src/cli.rs:1305:            verify_webhooks: self.verify_webhooks,
```

## rg: runner invocation paths
```bash
rg -n "run_flow|run_pack_with_options|run_provider_pack_flow" src/runner_exec.rs src/runner_integration.rs src/cli.rs src/providers.rs
```
```text
src/runner_exec.rs:26:pub fn run_provider_pack_flow(request: RunRequest) -> anyhow::Result<RunOutput> {
src/runner_exec.rs:52:    let result = greentic_runner_desktop::run_pack_with_options(&request.pack_path, opts)?;
src/runner_integration.rs:27:pub fn run_flow(
src/runner_integration.rs:33:    run_flow_with_options(
src/runner_integration.rs:48:pub fn run_flow_with_options(
src/providers.rs:87:        let output = runner_integration::run_flow(&runner, &pack_path, &setup_flow, &input)?;
src/providers.rs:98:                    runner_integration::run_flow(&runner, &pack_path, &verify_flow, &input)?;
src/cli.rs:1946:        let output = runner_integration::run_flow_with_options(
src/cli.rs:1983:        let output = runner_exec::run_provider_pack_flow(runner_exec::RunRequest {
```

## rg: .gtpack usage and provider roots
```bash
rg -n "providers/messaging|\.gtpack" src README.md
```
```text
README.md:13:# drop provider packs into providers/messaging/
README.md:49:- messaging: `providers/messaging/*.gtpack`
README.md:50:- events: `providers/events/*.gtpack`
src/providers.rs:188:    Ok(default_dir.join(format!("{provider}.gtpack")))
src/demo/build.rs:107:            if pack.ends_with(".gtpack") {
src/doctor.rs:179:        if pack.ends_with(".gtpack") {
src/demo/doctor.rs:12:        return Err(anyhow::anyhow!("No .gtpack files found in bundle."));
src/domains/mod.rs:44:            providers_dir: "providers/messaging",
src/domains/mod.rs:66:        Domain::Messaging => "validators-messaging.gtpack",
src/domains/mod.rs:67:        Domain::Events => "validators-events.gtpack",
src/domains/mod.rs:68:        Domain::Secrets => "validators-secrets.gtpack",
src/domains/mod.rs:125:                .strip_suffix(".gtpack")
src/tools/secrets.rs:274:            Path::new("pack.gtpack"),
src/tools/secrets.rs:288:                "pack.gtpack",
src/tools/secrets.rs:296:        let args = build_init_args("dev", "tenant1", None, Path::new("pack.gtpack"), false);
src/tools/secrets.rs:306:                "pack.gtpack",
```
