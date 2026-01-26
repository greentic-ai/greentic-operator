use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::dev_mode::DevSettings;

#[derive(Clone, Debug, Deserialize, Default)]
pub struct OperatorConfig {
    #[serde(default)]
    pub dev: Option<DevSettings>,
    #[serde(default)]
    pub services: Option<OperatorServicesConfig>,
    #[serde(default)]
    pub binaries: BTreeMap<String, String>,
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DomainEnabledMode {
    Auto,
    True,
    False,
}

impl Default for DomainEnabledMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl DomainEnabledMode {
    pub fn is_enabled(self, has_providers: bool) -> bool {
        match self {
            Self::Auto => has_providers,
            Self::True => true,
            Self::False => false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct OperatorServicesConfig {
    #[serde(default)]
    pub messaging: DomainServicesConfig,
    #[serde(default)]
    pub events: DomainServicesConfig,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct DomainServicesConfig {
    #[serde(default)]
    pub enabled: DomainEnabledMode,
    #[serde(default)]
    pub components: Vec<ServiceComponentConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServiceComponentConfig {
    pub id: String,
    pub binary: String,
    #[serde(default)]
    pub args: Vec<String>,
}

pub fn load_operator_config(root: &Path) -> anyhow::Result<Option<OperatorConfig>> {
    let path = root.join("greentic.yaml");
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path)?;
    if contents
        .lines()
        .all(|line| line.trim().is_empty() || line.trim().starts_with('#'))
    {
        return Ok(None);
    }
    let config: OperatorConfig = serde_yaml_bw::from_str(&contents)?;
    Ok(Some(config))
}

pub fn binary_override(
    config: Option<&OperatorConfig>,
    name: &str,
    config_dir: &Path,
) -> Option<PathBuf> {
    config.and_then(|config| config_binary_path(config, name, config_dir))
}

#[derive(Clone, Debug, Deserialize)]
pub struct DemoConfig {
    #[serde(default = "default_demo_tenant")]
    pub tenant: String,
    #[serde(default = "default_demo_team")]
    pub team: String,
    #[serde(default)]
    pub services: DemoServicesConfig,
    #[serde(default)]
    pub providers: Option<std::collections::BTreeMap<String, DemoProviderConfig>>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct DemoServicesConfig {
    #[serde(default)]
    pub nats: DemoNatsConfig,
    #[serde(default)]
    pub gateway: DemoGatewayConfig,
    #[serde(default)]
    pub egress: DemoEgressConfig,
    #[serde(default)]
    pub subscriptions: DemoSubscriptionsConfig,
    #[serde(default)]
    pub events: DemoEventsConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DemoNatsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_nats_url")]
    pub url: String,
    #[serde(default)]
    pub spawn: DemoNatsSpawnConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DemoNatsSpawnConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_nats_binary")]
    pub binary: String,
    #[serde(default = "default_nats_args")]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DemoGatewayConfig {
    #[serde(default = "default_gateway_binary")]
    pub binary: String,
    #[serde(default = "default_gateway_listen_addr")]
    pub listen_addr: String,
    #[serde(default = "default_gateway_port")]
    pub port: u16,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DemoEgressConfig {
    #[serde(default = "default_egress_binary")]
    pub binary: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct DemoSubscriptionsConfig {
    #[serde(default)]
    pub msgraph: DemoMsgraphSubscriptionsConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DemoEventsConfig {
    #[serde(default)]
    pub enabled: DomainEnabledMode,
    #[serde(default = "default_events_components")]
    pub components: Vec<ServiceComponentConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DemoMsgraphSubscriptionsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_msgraph_binary")]
    pub binary: String,
    #[serde(default = "default_msgraph_mode")]
    pub mode: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DemoProviderConfig {
    #[serde(default)]
    pub pack: Option<String>,
    #[serde(default)]
    pub setup_flow: Option<String>,
    #[serde(default)]
    pub verify_flow: Option<String>,
}

impl Default for DemoNatsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            url: default_nats_url(),
            spawn: DemoNatsSpawnConfig::default(),
        }
    }
}

impl Default for DemoNatsSpawnConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            binary: default_nats_binary(),
            args: default_nats_args(),
        }
    }
}

impl Default for DemoGatewayConfig {
    fn default() -> Self {
        Self {
            binary: default_gateway_binary(),
            listen_addr: default_gateway_listen_addr(),
            port: default_gateway_port(),
            args: Vec::new(),
        }
    }
}

impl Default for DemoEgressConfig {
    fn default() -> Self {
        Self {
            binary: default_egress_binary(),
            args: Vec::new(),
        }
    }
}

impl Default for DemoMsgraphSubscriptionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            binary: default_msgraph_binary(),
            mode: default_msgraph_mode(),
            args: Vec::new(),
        }
    }
}

impl Default for DemoEventsConfig {
    fn default() -> Self {
        Self {
            enabled: DomainEnabledMode::Auto,
            components: default_events_components(),
        }
    }
}

pub fn load_demo_config(path: &Path) -> anyhow::Result<DemoConfig> {
    let contents = std::fs::read_to_string(path)?;
    let config: DemoConfig = serde_yaml_bw::from_str(&contents)?;
    Ok(config)
}

fn config_binary_path(config: &OperatorConfig, name: &str, config_dir: &Path) -> Option<PathBuf> {
    config
        .binaries
        .get(name)
        .map(|value| resolve_path(config_dir, value))
}

fn resolve_path(base: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn default_demo_tenant() -> String {
    "demo".to_string()
}

fn default_demo_team() -> String {
    "default".to_string()
}

fn default_true() -> bool {
    true
}

fn default_nats_url() -> String {
    "nats://127.0.0.1:4222".to_string()
}

fn default_nats_binary() -> String {
    "nats-server".to_string()
}

fn default_nats_args() -> Vec<String> {
    vec!["-p".to_string(), "4222".to_string(), "-js".to_string()]
}

fn default_gateway_binary() -> String {
    "gsm-gateway".to_string()
}

fn default_gateway_listen_addr() -> String {
    "127.0.0.1".to_string()
}

fn default_gateway_port() -> u16 {
    8080
}

fn default_egress_binary() -> String {
    "gsm-egress".to_string()
}

fn default_msgraph_binary() -> String {
    "gsm-msgraph-subscriptions".to_string()
}

fn default_msgraph_mode() -> String {
    "poll".to_string()
}

pub(crate) fn default_events_components() -> Vec<ServiceComponentConfig> {
    vec![
        ServiceComponentConfig {
            id: "events-ingress".to_string(),
            binary: "greentic-events-ingress".to_string(),
            args: Vec::new(),
        },
        ServiceComponentConfig {
            id: "events-worker".to_string(),
            binary: "greentic-events-worker".to_string(),
            args: Vec::new(),
        },
    ]
}
