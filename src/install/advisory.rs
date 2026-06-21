//! 安装完成后的配置建议检查。
//!
//! 不同安装模式（单机 / 集群）关注的风险点不同，因此每种模式拥有独立的
//! `xxx_advisories()` 函数，仅共享 `path_overlaps` 这类纯路径判断逻辑。

use crate::config::InstallConfig;

/// 判断两个路径是否同一目录，或一个嵌套在另一个之内（去除末尾斜杠后逐段比较）。
pub fn path_overlaps(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.trim_end_matches('/').to_string();
    let a = norm(a);
    let b = norm(b);
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

/// 单机安装的配置建议：备份目录是否配置、备份/归档目录是否与数据目录混放等。
pub fn standalone_advisories(config: &InstallConfig, arch_path: &str) -> Vec<String> {
    let mut advisories: Vec<String> = Vec::new();

    match &config.backup_path {
        None => advisories.push(
            "未配置备份目录(backup_path)，单机部署建议同时配置归档与备份，避免数据丢失风险".to_string(),
        ),
        Some(backup_path) => {
            if path_overlaps(backup_path, &config.data_path) {
                advisories.push(format!(
                    "备份目录与数据目录位于同一路径（{} ⊂/= {}），建议备份至独立磁盘或目录，避免同盘故障导致数据与备份同时丢失",
                    backup_path, config.data_path
                ));
            }
        }
    }

    if path_overlaps(arch_path, &config.data_path) {
        advisories.push(format!(
            "归档目录与数据目录位于同一路径（{} ⊂/= {}），建议归档至独立磁盘或目录，避免同盘故障导致归档失效",
            arch_path, config.data_path
        ));
    }

    advisories
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(backup_path: Option<&str>, data_path: &str) -> InstallConfig {
        InstallConfig {
            backup_path: backup_path.map(str::to_string),
            data_path: data_path.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_path_overlaps_equal() {
        assert!(path_overlaps("/data", "/data"));
        assert!(path_overlaps("/data/", "/data"));
    }

    #[test]
    fn test_path_overlaps_nested() {
        assert!(path_overlaps("/data/arch", "/data"));
        assert!(path_overlaps("/data", "/data/arch"));
    }

    #[test]
    fn test_path_overlaps_distinct() {
        assert!(!path_overlaps("/data", "/backup"));
        assert!(!path_overlaps("/data2", "/data"));
    }

    #[test]
    fn test_standalone_advisories_missing_backup_path() {
        let cfg = make_config(None, "/opt/dmdbms/data");
        let advisories = standalone_advisories(&cfg, "/opt/dmdbms/data/arch");
        assert!(advisories.iter().any(|a| a.contains("未配置备份目录")));
    }

    #[test]
    fn test_standalone_advisories_backup_overlaps_data() {
        let cfg = make_config(Some("/opt/dmdbms/data"), "/opt/dmdbms/data");
        let advisories = standalone_advisories(&cfg, "/opt/dmdbms/data/arch");
        assert!(advisories.iter().any(|a| a.contains("备份目录与数据目录")));
    }

    #[test]
    fn test_standalone_advisories_arch_overlaps_data() {
        let cfg = make_config(Some("/mnt/backup"), "/opt/dmdbms/data");
        let advisories = standalone_advisories(&cfg, "/opt/dmdbms/data/arch");
        assert!(advisories.iter().any(|a| a.contains("归档目录与数据目录")));
    }

    #[test]
    fn test_standalone_advisories_clean_config_is_empty() {
        let cfg = make_config(Some("/mnt/backup"), "/opt/dmdbms/data");
        let advisories = standalone_advisories(&cfg, "/mnt/arch");
        assert!(advisories.is_empty());
    }
}
