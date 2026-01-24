use std::path::PathBuf;
use std::process::Command;

fn write_pack(path: &std::path::Path, pack_id: &str, entry_flows: &[&str]) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::FileOptions::<()>::default();
    zip.start_file("pack.manifest.json", options)?;
    let manifest = serde_json::json!({
        "meta": {
            "pack_id": pack_id,
            "entry_flows": entry_flows,
        }
    });
    std::io::Write::write_all(&mut zip, serde_json::to_string(&manifest)?.as_bytes())?;
    zip.finish()?;
    Ok(())
}

#[test]
fn demo_setup_runs_all_domains() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let messaging = root.join("providers").join("messaging");
    let events = root.join("providers").join("events");
    std::fs::create_dir_all(&messaging).unwrap();
    std::fs::create_dir_all(&events).unwrap();
    write_pack(&messaging.join("a.gtpack"), "msg-a", &["setup_default"]).unwrap();
    write_pack(&events.join("b.gtpack"), "evt-b", &["setup_default"]).unwrap();

    let status = Command::new(fake_bin("greentic-operator"))
        .args([
            "demo",
            "setup",
            "--bundle",
            root.to_string_lossy().as_ref(),
            "--tenant",
            "demo",
            "--domain",
            "all",
            "--runner-binary",
            fake_bin("fake_runner").to_string_lossy().as_ref(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let providers_root = root
        .join("state")
        .join("runtime")
        .join("demo")
        .join("providers");
    assert!(providers_root.join("msg-a.setup.json").exists());
    assert!(providers_root.join("evt-b.setup.json").exists());
}

#[test]
fn demo_setup_best_effort_skips_missing_setup() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let messaging = root.join("providers").join("messaging");
    std::fs::create_dir_all(&messaging).unwrap();
    write_pack(
        &messaging.join("good.gtpack"),
        "msg-good",
        &["setup_default"],
    )
    .unwrap();
    write_pack(&messaging.join("bad.gtpack"), "msg-bad", &["diagnostics"]).unwrap();

    let status = Command::new(fake_bin("greentic-operator"))
        .args([
            "demo",
            "setup",
            "--bundle",
            root.to_string_lossy().as_ref(),
            "--tenant",
            "demo",
            "--domain",
            "messaging",
            "--runner-binary",
            fake_bin("fake_runner").to_string_lossy().as_ref(),
            "--best-effort",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let providers_root = root
        .join("state")
        .join("runtime")
        .join("demo")
        .join("providers");
    assert!(providers_root.join("msg-good.setup.json").exists());
    assert!(!providers_root.join("msg-bad.setup.json").exists());
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
