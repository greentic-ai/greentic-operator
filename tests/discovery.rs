use std::fs::File;
use std::io::Write;
use std::path::Path;

use greentic_operator::discovery::{self, ProviderIdSource};

fn write_pack(path: &Path, pack_id: &str) -> anyhow::Result<()> {
    let file = File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::<()>::default();
    zip.start_file("pack.manifest.json", options)?;
    let manifest = serde_json::json!({
        "meta": {
            "pack_id": pack_id,
            "entry_flows": ["setup_default"],
        }
    });
    zip.write_all(serde_json::to_string(&manifest)?.as_bytes())?;
    zip.finish()?;
    Ok(())
}

fn write_pack_without_manifest(path: &Path) -> anyhow::Result<()> {
    let file = File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::<()>::default();
    zip.start_file("placeholder.txt", options)?;
    zip.write_all(b"placeholder")?;
    zip.finish()?;
    Ok(())
}

#[test]
fn discovery_detects_domains_and_manifest_ids() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let providers = root.join("providers");
    let messaging = providers.join("messaging");
    let events = providers.join("events");
    std::fs::create_dir_all(&messaging).unwrap();
    std::fs::create_dir_all(&events).unwrap();

    write_pack(&messaging.join("alpha.gtpack"), "messaging-alpha").unwrap();
    write_pack(&events.join("beta.gtpack"), "events-beta").unwrap();

    let result = discovery::discover(root).unwrap();
    assert!(result.domains.messaging);
    assert!(result.domains.events);
    assert_eq!(result.providers.len(), 2);
    assert_eq!(result.providers[0].id_source, ProviderIdSource::Manifest);
    assert_eq!(result.providers[1].id_source, ProviderIdSource::Manifest);
}

#[test]
fn discovery_falls_back_to_filename() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let events = root.join("providers").join("events");
    std::fs::create_dir_all(&events).unwrap();

    write_pack_without_manifest(&events.join("filename.gtpack")).unwrap();

    let result = discovery::discover(root).unwrap();
    assert!(!result.domains.messaging);
    assert!(result.domains.events);
    assert_eq!(result.providers.len(), 1);
    assert_eq!(result.providers[0].provider_id, "filename");
    assert_eq!(result.providers[0].id_source, ProviderIdSource::Filename);
}

#[test]
fn discovery_persists_outputs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let messaging = root.join("providers").join("messaging");
    std::fs::create_dir_all(&messaging).unwrap();
    write_pack(&messaging.join("alpha.gtpack"), "messaging-alpha").unwrap();

    let result = discovery::discover(root).unwrap();
    discovery::persist(root, "demo", &result).unwrap();

    let runtime = root.join("state").join("runtime").join("demo");
    let domains = std::fs::read_to_string(runtime.join("detected_domains.json")).unwrap();
    let providers = std::fs::read_to_string(runtime.join("detected_providers.json")).unwrap();
    let domains: serde_json::Value = serde_json::from_str(&domains).unwrap();
    let providers: serde_json::Value = serde_json::from_str(&providers).unwrap();
    assert_eq!(domains["messaging"], true);
    assert_eq!(providers.as_array().unwrap().len(), 1);
}
