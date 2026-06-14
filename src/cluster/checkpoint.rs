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
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let cp: Self = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("检查点文件格式错误，忽略: {}", e);
                return Ok(None);
            }
        };
        println!("[续] 检测到检查点，从上次进度继续安装");
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
}
