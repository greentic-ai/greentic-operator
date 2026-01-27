use std::collections::BTreeMap;
use std::path::PathBuf;

use greentic_operator::runtime_state::RuntimePaths;
use greentic_operator::supervisor::{
    ServiceId, ServiceSpec, read_status, spawn_service, stop_service,
};

#[test]
fn supervisor_spawns_and_stops_service() {
    let temp = tempfile::tempdir().unwrap();
    let state_dir = temp.path().join("state");
    let paths = RuntimePaths::new(&state_dir, "demo", "default");

    let bin = PathBuf::from(env!("CARGO_BIN_EXE_fake_service"));
    let spec = ServiceSpec {
        id: ServiceId::new("fake").unwrap(),
        argv: vec![bin.display().to_string(), "2".to_string()],
        cwd: None,
        env: BTreeMap::new(),
    };

    let handle = spawn_service(&paths, spec, None).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let log_contents = std::fs::read_to_string(handle.log_path).unwrap();
    assert!(log_contents.contains("ready"));

    let statuses = read_status(&paths).unwrap();
    assert_eq!(statuses.len(), 1);
    assert!(statuses[0].running);

    stop_service(&paths, &ServiceId::new("fake").unwrap(), 500).unwrap();
    let statuses = read_status(&paths).unwrap();
    assert!(statuses.is_empty());
}
