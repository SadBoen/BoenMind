#[test]
fn missing_config_uses_parent_data_directory() {
    let dir = tempfile::tempdir().expect("temp");
    let config_path = dir.path().join("config").join("mcp-context_mode.json");
    std::fs::create_dir_all(config_path.parent().expect("parent")).expect("mkdir");
    let config = super::Config::load(Some(&config_path)).expect("load");
    assert_eq!(config.data_dir, dir.path().join("context-mode"));
}

#[test]
fn accepts_ui_string_for_allowed_roots() {
    let dir = tempfile::tempdir().expect("temp");
    let config_path = dir.path().join("config.json");
    let raw = serde_json::json!({
        "data_dir": dir.path(),
        "allowed_roots": dir.path().to_string_lossy(),
    });
    std::fs::write(&config_path, serde_json::to_vec(&raw).expect("json")).expect("config");
    let config = super::Config::load(Some(&config_path)).expect("load");
    assert_eq!(config.allowed_roots.len(), 1);
    assert!(config.allows(dir.path()));
}

#[test]
fn execution_is_disabled_by_default() {
    let dir = tempfile::tempdir().expect("temp");
    let config_path = dir.path().join("config.json");
    std::fs::write(&config_path, b"{}").expect("config");
    let config = super::Config::load(Some(&config_path)).expect("load");
    assert!(!config.execution_enabled);
}
