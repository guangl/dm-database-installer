use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn file_name(oguid: u32) -> String {
    format!("dm_installer_dw_checkpoint_{oguid}.json")
}

/// 单节点安装进度标记。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeCheckpoint {
    #[serde(default)]
    pub env_setup_done: bool,
    #[serde(default)]
    pub uploaded: bool,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub db_inited: bool,
    #[serde(default)]
    pub synced: bool,
    #[serde(default)]
    pub config_distributed: bool,
    #[serde(default)]
    pub mount_started: bool,
    #[serde(default)]
    pub watcher_started: bool,
    #[serde(default)]
    pub monitor_started: bool,
    #[serde(default)]
    pub backup_configured: bool,
    #[serde(default)]
    pub sql_log_enabled: bool,
    #[serde(default)]
    pub param_tuned: bool,
}

/// 主备集群安装检查点：按节点 host 索引进度，集群级共享密码。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterCheckpoint {
    pub oguid: u32,
    pub sysdba_pwd: String,
    pub sysauditor_pwd: String,
    /// 自动下载/下载链接场景下缓存的安装包本地路径，所有节点共用同一份。
    #[serde(default)]
    pub package_cache: Option<String>,
    pub nodes: HashMap<String, NodeCheckpoint>,
}

impl ClusterCheckpoint {
    pub fn new(oguid: u32, sysdba_pwd: String, sysauditor_pwd: String, hosts: &[String]) -> Self {
        let nodes = hosts
            .iter()
            .map(|h| (h.clone(), NodeCheckpoint::default()))
            .collect();
        Self {
            oguid,
            sysdba_pwd,
            sysauditor_pwd,
            package_cache: None,
            nodes,
        }
    }

    pub fn node(&self, host: &str) -> NodeCheckpoint {
        self.nodes.get(host).cloned().unwrap_or_default()
    }

    pub fn mark<F: Fn(&mut NodeCheckpoint)>(&mut self, host: &str, f: F) {
        f(self.nodes.entry(host.to_string()).or_default());
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&cwd())
    }

    pub fn remove(oguid: u32) -> Result<()> {
        Self::remove_from(&cwd(), oguid)
    }

    pub(crate) fn save_to(&self, dir: &Path) -> Result<()> {
        let path = dir.join(file_name(self.oguid));
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub(crate) fn remove_from(dir: &Path, oguid: u32) -> Result<()> {
        let path = dir.join(file_name(oguid));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}

/// 从当前目录加载检查点（按 oguid 区分不同集群）。
pub fn load(oguid: u32) -> Result<Option<ClusterCheckpoint>> {
    load_from(&cwd(), oguid)
}

pub(crate) fn load_from(dir: &Path, oguid: u32) -> Result<Option<ClusterCheckpoint>> {
    let path = dir.join(file_name(oguid));
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let cp: ClusterCheckpoint = match serde_json::from_str(&content) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    if cp.oguid != oguid {
        return Ok(None);
    }
    crate::ui::log_info("[续] 检测到集群检查点，从上次进度继续安装");
    Ok(Some(cp))
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_cp() -> ClusterCheckpoint {
        ClusterCheckpoint::new(
            453331,
            "pwd1".into(),
            "pwd2".into(),
            &["192.168.1.10".to_string(), "192.168.1.11".to_string()],
        )
    }

    #[test]
    fn test_roundtrip_save_load() {
        let dir = TempDir::new().unwrap();
        let mut cp = make_cp();
        cp.mark("192.168.1.10", |n| n.installed = true);
        cp.save_to(dir.path()).unwrap();

        let loaded = load_from(dir.path(), 453331).unwrap().unwrap();
        assert_eq!(loaded.sysdba_pwd, "pwd1");
        assert!(loaded.node("192.168.1.10").installed);
        assert!(!loaded.node("192.168.1.11").installed);
    }

    #[test]
    fn test_load_returns_none_when_no_file() {
        let dir = TempDir::new().unwrap();
        assert!(load_from(dir.path(), 453331).unwrap().is_none());
    }

    #[test]
    fn test_load_ignores_mismatched_oguid() {
        let dir = TempDir::new().unwrap();
        make_cp().save_to(dir.path()).unwrap();
        assert!(load_from(dir.path(), 999).unwrap().is_none());
    }

    #[test]
    fn test_remove_deletes_file() {
        let dir = TempDir::new().unwrap();
        make_cp().save_to(dir.path()).unwrap();
        assert!(dir.path().join(file_name(453331)).exists());
        ClusterCheckpoint::remove_from(dir.path(), 453331).unwrap();
        assert!(!dir.path().join(file_name(453331)).exists());
    }

    #[test]
    fn test_load_ignores_corrupt_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(file_name(453331)), "not json").unwrap();
        assert!(load_from(dir.path(), 453331).unwrap().is_none());
    }

    #[test]
    fn test_node_returns_default_when_absent() {
        let cp = make_cp();
        assert!(!cp.node("192.168.1.99").installed);
    }
}
