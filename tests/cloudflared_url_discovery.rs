use std::path::PathBuf;

use greentic_operator::cloudflared::{CloudflaredConfig, start_quick_tunnel};
use greentic_operator::runtime_state::RuntimePaths;

#[test]
fn cloudflared_discovers_public_url() {
    let temp = tempfile::tempdir().unwrap();
    let binary = resolve_fake_cloudflared();
    let config = CloudflaredConfig {
        binary,
        local_port: 8080,
        extra_args: Vec::new(),
        restart: true,
    };
    let paths = RuntimePaths::new(temp.path(), "demo", "default");
    let log_path = temp.path().join("logs").join("cloudflared.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let handle = start_quick_tunnel(&paths, &config, &log_path).unwrap();
    assert_eq!(handle.log_path, log_path);
    assert_eq!(handle.url, "https://example.trycloudflare.com");

    let url_path = greentic_operator::cloudflared::public_url_path(&paths);
    let persisted = std::fs::read_to_string(url_path).unwrap();
    assert_eq!(persisted.trim(), "https://example.trycloudflare.com");
}

fn resolve_fake_cloudflared() -> PathBuf {
    if let Ok(value) = std::env::var("CARGO_BIN_EXE_fake_cloudflared") {
        return PathBuf::from(value);
    }
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.file_name().and_then(|name| name.to_str()) == Some("deps") {
        path.pop();
    }
    path.push(binary_name("fake_cloudflared"));
    path
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}
