use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::install::checkpoint_io::{cwd, load_json_from, remove_file_in, save_json_to};

const FILE_NAME: &str = "dm_installer_checkpoint.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub install_path: String,
    pub sysdba_pwd: String,
    pub sysauditor_pwd: String,
    #[serde(default)]
    pub package_cache: Option<String>,
    #[serde(default)]
    pub uploaded: bool,
    #[serde(default)]
    pub env_setup_done: bool,
    pub installed: bool,
    #[serde(default)]
    pub db_inited: bool,
    #[serde(default)]
    pub arch_configured: bool,
    #[serde(default)]
    pub services_done: bool,
    #[serde(default)]
    pub backup_configured: bool,
    #[serde(default)]
    pub param_tuned: bool,
    #[serde(default)]
    pub sql_log_enabled: bool,
}

impl Checkpoint {
    pub fn new(install_path: &str, sysdba_pwd: String, sysauditor_pwd: String) -> Self {
        Self {
            install_path: install_path.to_string(),
            sysdba_pwd,
            sysauditor_pwd,
            package_cache: None,
            uploaded: false,
            env_setup_done: false,
            installed: false,
            db_inited: false,
            arch_configured: false,
            services_done: false,
            backup_configured: false,
            param_tuned: false,
            sql_log_enabled: false,
        }
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&cwd())
    }

    pub fn remove() -> Result<()> {
        Self::remove_from(&cwd())
    }

    pub(crate) fn save_to(&self, dir: &Path) -> Result<()> {
        save_json_to(dir, FILE_NAME, self)
    }

    pub(crate) fn remove_from(dir: &Path) -> Result<()> {
        remove_file_in(dir, FILE_NAME)
    }
}

/// 从当前目录加载检查点；install_path 不匹配时忽略。
pub fn load(install_path: &str) -> Result<Option<Checkpoint>> {
    load_from(&cwd(), install_path)
}

pub(crate) fn load_from(dir: &Path, install_path: &str) -> Result<Option<Checkpoint>> {
    let cp: Option<Checkpoint> = load_json_from(dir, FILE_NAME)?;
    let Some(cp) = cp else {
        return Ok(None);
    };
    if cp.install_path != install_path {
        return Ok(None);
    }
    println!("[续] 检测到检查点，从上次进度继续安装");
    Ok(Some(cp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_cp(install_path: &str) -> Checkpoint {
        Checkpoint::new(install_path, "pwd1".into(), "pwd2".into())
    }

    #[test]
    fn test_roundtrip_save_load() {
        let dir = TempDir::new().unwrap();
        let mut cp = make_cp("/opt/dmdbms");
        cp.installed = true;
        cp.save_to(dir.path()).unwrap();

        let loaded = load_from(dir.path(), "/opt/dmdbms").unwrap().unwrap();
        assert_eq!(loaded.install_path, "/opt/dmdbms");
        assert_eq!(loaded.sysdba_pwd, "pwd1");
        assert!(loaded.installed);
    }

    #[test]
    fn test_load_returns_none_when_no_file() {
        let dir = TempDir::new().unwrap();
        assert!(load_from(dir.path(), "/opt/dmdbms").unwrap().is_none());
    }

    #[test]
    fn test_load_ignores_mismatched_install_path() {
        let dir = TempDir::new().unwrap();
        make_cp("/opt/other").save_to(dir.path()).unwrap();
        assert!(load_from(dir.path(), "/opt/dmdbms").unwrap().is_none());
    }

    #[test]
    fn test_remove_deletes_file() {
        let dir = TempDir::new().unwrap();
        make_cp("/opt/dmdbms").save_to(dir.path()).unwrap();
        assert!(dir.path().join(FILE_NAME).exists());
        Checkpoint::remove_from(dir.path()).unwrap();
        assert!(!dir.path().join(FILE_NAME).exists());
    }

    #[test]
    fn test_load_ignores_corrupt_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "not json").unwrap();
        assert!(load_from(dir.path(), "/opt/dmdbms").unwrap().is_none());
    }
}
