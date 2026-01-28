use std::path::Path;

use crate::discovery;
use crate::domains::{self, Domain};

pub struct ProviderComponent {
    pub provider_id: String,
    pub pack: domains::ProviderPack,
}

pub fn resolve_provider_component(
    bundle: &Path,
    provider: &str,
) -> anyhow::Result<ProviderComponent> {
    domains::ensure_cbor_packs(bundle)?;
    let discovery =
        discovery::discover_with_options(bundle, discovery::DiscoveryOptions { cbor_only: true })?;
    let packs = domains::discover_provider_packs(bundle, Domain::Messaging)?;
    for pack in packs {
        if pack.pack_id == provider || pack.file_name == format!("{provider}.gtpack") {
            return Ok(ProviderComponent {
                provider_id: provider.to_string(),
                pack,
            });
        }
        let provider_map = discovery
            .providers
            .iter()
            .find(|entry| entry.pack_path == pack.path);
        if let Some(map_entry) = provider_map
            && map_entry.provider_id == provider
        {
            return Ok(ProviderComponent {
                provider_id: map_entry.provider_id.clone(),
                pack,
            });
        }
    }
    Err(anyhow::anyhow!("provider pack not found for {}", provider))
}
