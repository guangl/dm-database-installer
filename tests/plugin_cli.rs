use std::process::{Command, Output};

fn plugin(args: &[&str], directory: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dm-installer"))
        .args(args)
        .current_dir(directory)
        .env("DM_PLUGIN_API_VERSION", "1")
        .env("DM_PLUGIN_CAPABILITIES", "config-dirs-v1")
        .env("DM_PLUGIN_DIR", directory.join("binary"))
        .env("DM_PLUGIN_HOME", directory.join("host"))
        .env("DM_PLUGIN_CONFIG_DIR", directory.join("config"))
        .env("DM_PLUGIN_DATA_DIR", directory.join("data"))
        .env("DM_PLUGIN_CACHE_DIR", directory.join("cache"))
        .output()
        .unwrap()
}

#[test]
fn test_plugin_help_and_usage_exit_codes() {
    let directory = tempfile::tempdir().unwrap();
    let output = plugin(&["--help"], directory.path());
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("dm installer"));
    let output = plugin(&["invalid-command"], directory.path());
    assert_eq!(output.status.code(), Some(2));
    assert!(!output.stderr.is_empty());
}

#[test]
fn test_plugin_config_stays_in_working_directory() {
    let directory = tempfile::tempdir().unwrap();
    let output = plugin(&["init", "standalone"], directory.path());
    assert!(output.status.success(), "{:?}", output);
    assert!(directory.path().join("config.toml").is_file());
    assert!(directory.path().join("standalone.toml").is_file());
    assert!(!directory.path().join("config").exists());
    let output = plugin(&["validate"], directory.path());
    assert!(output.status.success(), "{:?}", output);
}

#[test]
fn test_plugin_blocks_self_update_before_network_or_replacement() {
    let directory = tempfile::tempdir().unwrap();
    for args in [vec!["self-update"], vec!["self-update", "--check"]] {
        let output = plugin(&args, directory.path());
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("dm update installer"));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn test_plugin_rejects_unsupported_protocol() {
    let output = Command::new(env!("CARGO_BIN_EXE_dm-installer"))
        .arg("--help")
        .env("DM_PLUGIN_API_VERSION", "999")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unsupported host plugin API"));
}

#[test]
fn test_manifest_version_matches_package() {
    let manifest: toml::Value = toml::from_str(include_str!("../dm-plugin.toml")).unwrap();
    assert_eq!(
        manifest["version"].as_str(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(manifest["name"].as_str(), Some("installer"));
    assert_eq!(manifest["api_version"].as_integer(), Some(1));
}
