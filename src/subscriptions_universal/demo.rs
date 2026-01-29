use std::path::{Path, PathBuf};

use anyhow::Result;
use uuid::Uuid;

use crate::cli::{discovery_map, provider_id_for_pack, resolve_demo_provider_pack};
use crate::config::DemoDesiredSubscription;
use crate::demo::runner_host::{DemoRunnerHost, OperatorContext};
use crate::discovery;
use crate::domains::Domain;
use crate::subscriptions_universal::scheduler::Scheduler;
use crate::subscriptions_universal::{AuthUserRefV1, SubscriptionEnsureRequest};

pub fn state_root(bundle: &Path) -> PathBuf {
    bundle.join("state").join("subscriptions")
}

pub fn build_runner(
    bundle: &Path,
    tenant: &str,
    team: Option<String>,
) -> Result<(DemoRunnerHost, OperatorContext)> {
    let discovery =
        discovery::discover_with_options(bundle, discovery::DiscoveryOptions { cbor_only: true })?;
    let runner_host = DemoRunnerHost::new(
        bundle.to_path_buf(),
        &discovery,
        None,
        crate::secrets_gate::default_manager(),
        false,
    )?;
    let context = OperatorContext {
        tenant: tenant.to_string(),
        team,
        correlation_id: None,
    };
    Ok((runner_host, context))
}

pub fn ensure_desired_subscriptions(
    bundle: &Path,
    tenant: &str,
    team: Option<String>,
    desired: &[DemoDesiredSubscription],
    scheduler: &Scheduler<DemoRunnerHost>,
) -> Result<()> {
    if desired.is_empty() {
        return Ok(());
    }
    let team_ref = team.as_deref();
    for entry in desired {
        let pack = resolve_demo_provider_pack(
            bundle,
            tenant,
            team_ref,
            &entry.provider,
            Domain::Messaging,
        )?;
        let discovery = discovery::discover_with_options(
            bundle,
            discovery::DiscoveryOptions { cbor_only: true },
        )?;
        let provider_map = discovery_map(&discovery.providers);
        let provider_id = provider_id_for_pack(&pack.path, &pack.pack_id, Some(&provider_map));
        let binding_id = entry
            .binding_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let request = to_request(entry, &binding_id);
        scheduler.ensure_once(&provider_id, &request)?;
    }
    Ok(())
}

fn to_request(entry: &DemoDesiredSubscription, binding_id: &str) -> SubscriptionEnsureRequest {
    SubscriptionEnsureRequest {
        binding_id: binding_id.to_string(),
        resource: Some(entry.resource.clone()),
        change_types: if entry.change_types.is_empty() {
            vec!["created".to_string()]
        } else {
            entry.change_types.clone()
        },
        notification_url: entry.notification_url.clone(),
        client_state: entry.client_state.clone(),
        user: entry.user.as_ref().map(|value| AuthUserRefV1 {
            user_id: value.user_id.clone(),
            token_key: value.token_key.clone(),
            tenant_id: None,
            email: None,
            display_name: None,
        }),
        expiration_target_unix_ms: None,
    }
}
