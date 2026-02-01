use std::env;
use std::fmt;

use crate::domains::{self, Domain};

#[derive(Clone, Copy, Debug)]
pub enum ConfigValueSource {
    Argument(&'static str),
    Platform(&'static str),
    Derived(&'static str),
}

impl fmt::Display for ConfigValueSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Argument(detail) => write!(f, "arg({detail})"),
            Self::Platform(detail) => write!(f, "platform({detail})"),
            Self::Derived(detail) => write!(f, "derived({detail})"),
        }
    }
}

pub struct ConfigGateItem {
    pub name: String,
    pub value: Option<String>,
    pub required: bool,
    pub source: ConfigValueSource,
}

impl ConfigGateItem {
    pub fn new(
        name: impl Into<String>,
        value: Option<String>,
        source: ConfigValueSource,
        required: bool,
    ) -> Self {
        Self {
            name: name.into(),
            value,
            required,
            source,
        }
    }
}

pub fn log_config_gate(
    domain: Domain,
    tenant: &str,
    team: Option<&str>,
    env: &str,
    items: &[ConfigGateItem],
) {
    if !debug_enabled() || items.is_empty() {
        return;
    }
    let team_label = team.unwrap_or("default");
    eprintln!(
        "config_gate::domain={} tenant={} team={} env={} items:",
        domains::domain_name(domain),
        tenant,
        team_label,
        env
    );
    for item in items {
        let value = item.value.as_deref().unwrap_or("<missing>");
        let required = if item.required {
            "required"
        } else {
            "optional"
        };
        eprintln!(
            "config_gate::  - {}={} [{}] {}",
            item.name, value, item.source, required
        );
    }
}

fn debug_enabled() -> bool {
    matches!(
        env::var("GREENTIC_OPERATOR_DEMO_DEBUG").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}
