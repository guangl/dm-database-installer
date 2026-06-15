use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "dm_cluster_checkpoint.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClusterCheckpoint {
    #[serde(default)]
    pub preflight_done: bool,
    #[serde(default)]
    pub install_done: bool,
    #[serde(default)]
    pub primary_init_done: bool,
    #[serde(default)]
    pub backup_done: bool,
    #[serde(default)]
    pub standby_restore_done: bool,
}

impl ClusterCheckpoint {
    pub fn save(&self) -> Result<()> {
        self.save_to(&cwd())
    }

    pub fn load() -> Result<Option<Self>> {
        Self::load_from(&cwd())
    }

    pub fn remove() -> Result<()> {
        Self::remove_from(&cwd())
    }

    pub(crate) fn save_to(&self, dir: &Path) -> Result<()> {
        let path = dir.join(FILE_NAME);
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        tracing::debug!("检查点已保存: {}", path.display());
        Ok(())
    }

    pub(crate) fn load_from(dir: &Path) -> Result<Option<Self>> {
        let path = dir.join(FILE_NAME);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let cp: Self = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("检查点文件格式错误，忽略: {}", e);
                return Ok(None);
            }
        };
        tracing::info!("[续] 检测到检查点，从上次进度继续安装");
        Ok(Some(cp))
    }

    pub(crate) fn remove_from(dir: &Path) -> Result<()> {
        let path = dir.join(FILE_NAME);
        if path.exists() {
            std::fs::remove_file(&path)?;
            tracing::debug!("检查点已删除");
        }
        Ok(())
    }
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cp = ClusterCheckpoint {
            preflight_done: true,
            install_done: true,
            primary_init_done: false,
            backup_done: true,
            standby_restore_done: false,
        };
        cp.save_to(dir.path()).unwrap();

        let loaded = ClusterCheckpoint::load_from(dir.path()).unwrap().unwrap();
        assert!(loaded.preflight_done);
        assert!(loaded.install_done);
        assert!(!loaded.primary_init_done);
        assert!(loaded.backup_done);
        assert!(!loaded.standby_restore_done);
    }

    #[test]
    fn test_load_returns_none() {
        let dir = TempDir::new().unwrap();
        let result = ClusterCheckpoint::load_from(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_remove_deletes_file() {
        let dir = TempDir::new().unwrap();
        let cp = ClusterCheckpoint::default();
        cp.save_to(dir.path()).unwrap();
        assert!(dir.path().join("dm_cluster_checkpoint.json").exists());
        ClusterCheckpoint::remove_from(dir.path()).unwrap();
        assert!(!dir.path().join("dm_cluster_checkpoint.json").exists());
    }

    #[test]
    fn test_load_ignores_corrupt() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("dm_cluster_checkpoint.json"), "not json").unwrap();
        let result = ClusterCheckpoint::load_from(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_dsc_checkpoint_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cp = ClusterCheckpoint {
            dsc_config_distributed: true,
            css_asm_started: true,
            asm_diskgroup_created: true,
            dminit_shared_done: false,
            config_dir_distributed: false,
            dmserver_started: false,
            ..Default::default()
        };
        cp.save_to(dir.path()).unwrap();
        let loaded = ClusterCheckpoint::load_from(dir.path()).unwrap().unwrap();
        assert!(loaded.dsc_config_distributed, "dsc_config_distributed 应为 true");
        assert!(loaded.css_asm_started, "css_asm_started 应为 true");
        assert!(loaded.asm_diskgroup_created, "asm_diskgroup_created 应为 true");
        assert!(!loaded.dminit_shared_done, "dminit_shared_done 应为 false");
        assert!(!loaded.config_dir_distributed, "config_dir_distributed 应为 false");
        assert!(!loaded.dmserver_started, "dmserver_started 应为 false");
    }

    #[test]
    fn test_old_checkpoint_file_still_loads() {
        let dir = TempDir::new().unwrap();
        // 旧版 JSON 仅包含 5 个字段，无 DSC 字段
        let old_json = r#"{"preflight_done":true,"install_done":true,"primary_init_done":false,"backup_done":false,"standby_restore_done":false}"#;
        std::fs::write(dir.path().join("dm_cluster_checkpoint.json"), old_json).unwrap();
        let cp = ClusterCheckpoint::load_from(dir.path()).unwrap().unwrap();
        // 旧字段应保留
        assert!(cp.preflight_done, "preflight_done 应为 true");
        assert!(cp.install_done, "install_done 应为 true");
        assert!(!cp.primary_init_done, "primary_init_done 应为 false");
        // DSC 字段应默认 false（#[serde(default)] 生效）
        assert!(!cp.dsc_config_distributed, "dsc_config_distributed 应默认 false");
        assert!(!cp.css_asm_started, "css_asm_started 应默认 false");
        assert!(!cp.asm_diskgroup_created, "asm_diskgroup_created 应默认 false");
        assert!(!cp.dminit_shared_done, "dminit_shared_done 应默认 false");
        assert!(!cp.config_dir_distributed, "config_dir_distributed 应默认 false");
        assert!(!cp.dmserver_started, "dmserver_started 应默认 false");
    }
}
