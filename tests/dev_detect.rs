use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use greentic_operator::dev_detect::{DetectOptions, detect_repo_map, is_on_path, merge_repo_map};
use greentic_operator::dev_mode::DevProfile;

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn touch_executable(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "stub").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }
}

#[test]
fn detect_repo_map_handles_ambiguity() {
    let temp = tempfile::tempdir().unwrap();
    let repo_a = temp.path().join("repoA/target/debug");
    let repo_b = temp.path().join("repoB/target/debug");
    let repo_c = temp.path().join("repoC/target/debug");
    touch_executable(&repo_a.join(binary_name("foo")));
    touch_executable(&repo_b.join(binary_name("bar")));
    touch_executable(&repo_c.join(binary_name("foo")));

    let result = detect_repo_map(&DetectOptions {
        root: temp.path().to_path_buf(),
        profile: DevProfile::Debug,
    })
    .unwrap();

    assert_eq!(result.unambiguous.get("bar"), Some(&"repoB".to_string()));
    assert!(!result.unambiguous.contains_key("foo"));
    let repos = result.ambiguous.get("foo").unwrap();
    assert!(repos.contains(&"repoA".to_string()));
    assert!(repos.contains(&"repoC".to_string()));
}

#[test]
fn merge_preserves_existing_mappings() {
    let temp = tempfile::tempdir().unwrap();
    let repo_b = temp.path().join("repoB/target/debug");
    touch_executable(&repo_b.join(binary_name("bar")));

    let result = detect_repo_map(&DetectOptions {
        root: temp.path().to_path_buf(),
        profile: DevProfile::Debug,
    })
    .unwrap();

    let mut existing = BTreeMap::from([("bar".to_string(), "repoZ".to_string())]);
    let summary = merge_repo_map(&mut existing, &result, true);
    assert_eq!(existing.get("bar"), Some(&"repoZ".to_string()));
    assert_eq!(summary.skipped_mapped, 1);
    assert_eq!(summary.added, 0);
}

#[test]
fn merge_includes_binaries_even_if_on_path() {
    let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let repo_b = temp.path().join("repoB/target/debug");
    touch_executable(&repo_b.join(binary_name("bar")));

    let result = detect_repo_map(&DetectOptions {
        root: temp.path().to_path_buf(),
        profile: DevProfile::Debug,
    })
    .unwrap();

    let bin_dir = temp.path().join("pathbin");
    touch_executable(&bin_dir.join(binary_name("bar")));
    let original_path = std::env::var("PATH").unwrap_or_default();
    let joined = std::env::join_paths([bin_dir.clone()])
        .unwrap()
        .to_string_lossy()
        .to_string();
    unsafe {
        std::env::set_var("PATH", joined);
    }

    assert!(is_on_path("bar"));
    let mut existing = BTreeMap::new();
    let summary = merge_repo_map(&mut existing, &result, false);
    assert_eq!(existing.get("bar"), Some(&"repoB".to_string()));
    assert_eq!(summary.added, 1);

    unsafe {
        std::env::set_var("PATH", original_path);
    }
}
