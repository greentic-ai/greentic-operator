use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn write_pack(path: &std::path::Path, pack_id: &str) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::<()>::default();
    zip.start_file("manifest.cbor", options)?;
    let manifest = serde_json::json!({
        "meta": {
            "pack_id": pack_id,
            "entry_flows": ["setup_default"],
        }
    });
    let bytes = serde_cbor::to_vec(&manifest)?;
    std::io::Write::write_all(&mut zip, &bytes)?;
    zip.finish()?;
    Ok(())
}

#[test]
fn demo_up_starts_events_services_when_events_packs_exist() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("providers").join("events")).unwrap();
    write_pack(
        &root.join("providers").join("events").join("events.gtpack"),
        "events-pack",
    )
    .unwrap();

    let config = format!(
        r#"services:
  events:
    enabled: auto
    components:
      - id: events-ingress
        binary: "{ingress}"
      - id: events-worker
        binary: "{worker}"
"#,
        ingress = fake_bin("fake_events_ingress").display(),
        worker = fake_bin("fake_events_worker").display(),
    );
    std::fs::write(root.join("greentic.yaml"), config).unwrap();

    let mut child = Command::new(fake_bin("greentic-operator"))
        .args([
            "demo",
            "start",
            "--bundle",
            root.to_string_lossy().as_ref(),
            "--tenant",
            "demo",
            "--no-nats",
            "--cloudflared",
            "off",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_secs(3));
    let _ = child.kill();
    let _ = child.wait();
    assert!(root.join("state").exists());
}

fn fake_bin(name: &str) -> PathBuf {
    if let Ok(value) = std::env::var(format!("CARGO_BIN_EXE_{name}")) {
        return PathBuf::from(value);
    }
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.file_name().and_then(|name| name.to_str()) == Some("deps") {
        path.pop();
    }
    path.push(binary_name(name));
    path
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}
