use anyhow::Result;

use crate::config::InstallConfig;

/// 检测 install_path 下是否已有达梦实例（通过 dm.ini 存在性判断）。
/// 返回 Ok(true) 表示实例已存在，Ok(false) 表示未安装。
pub fn check_existing_instance(config: &InstallConfig) -> Result<bool> {
    todo!("RED 阶段占位，实现在 GREEN 阶段填入")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_config_with_path(install_path: &str) -> InstallConfig {
        InstallConfig {
            install_path: install_path.to_string(),
            ..InstallConfig::default()
        }
    }

    #[test]
    fn test_no_existing_instance() {
        let dir = TempDir::new().unwrap();
        let config = make_config_with_path(dir.path().to_str().unwrap());
        // dm.ini 不存在，应返回 false
        assert_eq!(
            check_existing_instance(&config).unwrap(),
            false,
            "无 dm.ini 时应返回 false"
        );
    }

    #[test]
    fn test_existing_instance_detected() {
        let dir = TempDir::new().unwrap();
        // 创建 dm.ini 模拟已有实例
        fs::write(dir.path().join("dm.ini"), "").unwrap();
        let config = make_config_with_path(dir.path().to_str().unwrap());
        assert_eq!(
            check_existing_instance(&config).unwrap(),
            true,
            "dm.ini 存在时应返回 true"
        );
    }
}
