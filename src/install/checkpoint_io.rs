use anyhow::Result;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

/// 将值序列化为 JSON 并写入 `dir/file_name`，供单机/集群 checkpoint 共用。
pub(crate) fn save_json_to<T: Serialize>(dir: &Path, file_name: &str, value: &T) -> Result<()> {
    let path = dir.join(file_name);
    let content = serde_json::to_string_pretty(value)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// 删除 `dir/file_name`（不存在时视为成功）。
pub(crate) fn remove_file_in(dir: &Path, file_name: &str) -> Result<()> {
    let path = dir.join(file_name);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// 读取并反序列化 `dir/file_name`；文件不存在或内容损坏均返回 `None`，不视为错误。
pub(crate) fn load_json_from<T: DeserializeOwned>(dir: &Path, file_name: &str) -> Result<Option<T>> {
    let path = dir.join(file_name);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content).ok())
}

pub(crate) fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use tempfile::TempDir;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Sample {
        value: u32,
    }

    #[test]
    fn test_roundtrip_save_load() {
        let dir = TempDir::new().unwrap();
        save_json_to(dir.path(), "sample.json", &Sample { value: 42 }).unwrap();
        let loaded: Option<Sample> = load_json_from(dir.path(), "sample.json").unwrap();
        assert_eq!(loaded, Some(Sample { value: 42 }));
    }

    #[test]
    fn test_load_returns_none_when_missing() {
        let dir = TempDir::new().unwrap();
        let loaded: Option<Sample> = load_json_from(dir.path(), "missing.json").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_load_returns_none_when_corrupt() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("sample.json"), "not json").unwrap();
        let loaded: Option<Sample> = load_json_from(dir.path(), "sample.json").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_remove_file_in_is_idempotent() {
        let dir = TempDir::new().unwrap();
        save_json_to(dir.path(), "sample.json", &Sample { value: 1 }).unwrap();
        remove_file_in(dir.path(), "sample.json").unwrap();
        assert!(!dir.path().join("sample.json").exists());
        remove_file_in(dir.path(), "sample.json").unwrap();
    }
}
