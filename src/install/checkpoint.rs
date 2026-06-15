use anyhow::Result;
use serde::{Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "dm_checkpoint.json";

pub fn save<T: Serialize>(value: &T) -> Result<()> {
    save_to(value, &cwd())
}

pub fn save_to<T: Serialize>(value: &T, dir: &Path) -> Result<()> {
    let path = dir.join(FILE_NAME);
    let content = serde_json::to_string_pretty(value)?;
    std::fs::write(&path, content)?;
    tracing::debug!("检查点已保存: {}", path.display());
    Ok(())
}

pub fn load<T: Default + DeserializeOwned>() -> Result<Option<T>> {
    load_from(&cwd())
}

pub fn load_from<T: Default + DeserializeOwned>(dir: &Path) -> Result<Option<T>> {
    let path = dir.join(FILE_NAME);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    match serde_json::from_str(&content) {
        Ok(cp) => {
            tracing::info!("[续] 检测到检查点，从上次进度继续安装");
            Ok(Some(cp))
        }
        Err(e) => {
            tracing::warn!("检查点文件格式错误，忽略: {}", e);
            Ok(None)
        }
    }
}

pub fn remove() -> Result<()> {
    remove_from(&cwd())
}

pub fn remove_from(dir: &Path) -> Result<()> {
    let path = dir.join(FILE_NAME);
    if path.exists() {
        std::fs::remove_file(&path)?;
        tracing::debug!("检查点已删除");
    }
    Ok(())
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
