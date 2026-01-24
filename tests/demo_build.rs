use std::fs;

use serde_yaml_bw::Value;

#[test]
fn demo_build_writes_bundle_with_relative_paths() {
    let temp = tempfile::tempdir().unwrap();
    let project_root = temp.path().join("project");

    fs::create_dir_all(project_root.join("providers").join("messaging")).unwrap();
    fs::create_dir_all(project_root.join("packs").join("pack1")).unwrap();
    fs::write(
        project_root
            .join("providers")
            .join("messaging")
            .join("provider.gtpack"),
        "pack",
    )
    .unwrap();
    fs::write(project_root.join("packs").join("pack2.gtpack"), "pack").unwrap();

    fs::create_dir_all(project_root.join("tenants").join("alpha")).unwrap();
    fs::write(
        project_root
            .join("tenants")
            .join("alpha")
            .join("tenant.gmap"),
        "_ = forbidden\n",
    )
    .unwrap();

    let old_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(temp.path()).unwrap();
    greentic_operator::project::sync_project(std::path::Path::new("project")).unwrap();
    std::env::set_current_dir(old_dir).unwrap();

    let bundle_root = temp.path().join("demo-bundle");
    unsafe {
        std::env::set_var("GREENTIC_OPERATOR_SKIP_DOCTOR", "1");
    }

    greentic_operator::demo::build_bundle(
        &project_root,
        greentic_operator::demo::BuildOptions {
            out_dir: bundle_root.clone(),
            tenant: Some("alpha".to_string()),
            team: None,
            allow_pack_dirs: true,
            only_used_providers: false,
            run_doctor: false,
        },
        None,
    )
    .unwrap();

    unsafe {
        std::env::remove_var("GREENTIC_OPERATOR_SKIP_DOCTOR");
    }

    let manifest = fs::read_to_string(bundle_root.join("resolved").join("alpha.yaml")).unwrap();
    let value: Value = serde_yaml_bw::from_str(&manifest).unwrap();
    assert_eq!(value.get("project_root").unwrap().as_str().unwrap(), "./");
    let packs = value.get("packs").unwrap().as_sequence().unwrap();
    assert!(
        packs
            .iter()
            .any(|item| item.as_str() == Some("packs/pack1"))
    );
    assert!(
        packs
            .iter()
            .any(|item| item.as_str() == Some("packs/pack2.gtpack"))
    );
}
