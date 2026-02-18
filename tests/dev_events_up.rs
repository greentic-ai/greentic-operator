use std::path::PathBuf;
use std::process::Command;

fn write_pack(path: &std::path::Path, pack_id: &str) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::<()>::default();
    zip.start_file("pack.manifest.json", options)?;
    let manifest = serde_json::json!({
        "meta": {
            "pack_id": pack_id,
            "entry_flows": ["setup_default"],
        }
    });
    std::io::Write::write_all(&mut zip, serde_json::to_string(&manifest)?.as_bytes())?;
    zip.finish()?;
    Ok(())
}

#[test]
fn dev_up_starts_events_only_when_events_packs_present() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let providers = root.join("providers").join("events");
    std::fs::create_dir_all(&providers).unwrap();
    write_pack(&providers.join("events.gtpack"), "events-pack").unwrap();

    let config = r#"services:
  events:
    enabled: auto
"#;
    std::fs::write(root.join("greentic.yaml"), config).unwrap();

    let output = Command::new(fake_bin("greentic-operator"))
        .args([
            "dev",
            "up",
            "--tenant",
            "demo",
            "--project-root",
            root.to_string_lossy().as_ref(),
            "--no-nats",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("events: enabled (in-process via operator ingress + timer scheduler)"));
    assert!(!stdout.contains("events-ingress: Started"));
    assert!(!stdout.contains("events-worker: Started"));

    let messaging_pid = root.join("state").join("pids").join("messaging-demo.pid");
    assert!(!messaging_pid.exists());

    let _ = Command::new(fake_bin("greentic-operator"))
        .args([
            "dev",
            "down",
            "--tenant",
            "demo",
            "--project-root",
            root.to_string_lossy().as_ref(),
            "--no-nats",
        ])
        .status();
}

fn fake_bin(name: &str) -> PathBuf {
    if name == "greentic-operator" {
        return PathBuf::from(env!("CARGO_BIN_EXE_greentic-operator"));
    }
    example_bin(name)
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn example_bin(name: &str) -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.file_name().and_then(|name| name.to_str()) == Some("deps") {
        path.pop();
    }
    let candidate = path.join("examples").join(binary_name(name));
    if candidate.exists() {
        return candidate;
    }
    let status = Command::new("cargo")
        .args(["build", "--example", name])
        .status()
        .expect("failed to build example binary");
    assert!(status.success(), "failed to build example binary");
    candidate
}
