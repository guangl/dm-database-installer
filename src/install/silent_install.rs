use anyhow::{Context, Result};
use std::{io::Write, path::Path, process::Command};
use tempfile::NamedTempFile;

use crate::config::InstallConfig;

/// 执行 DM 静默安装：生成 XML 响应文件 + 调用 DMInstall.bin -q。
pub fn run(config: &InstallConfig, extract_dir: &Path) -> Result<()> {
    todo!("Task 1 RED: 待实现 silent_install::run")
}

/// 生成 DM 安装 XML 响应文件（含 XML 转义防注入）。
pub(crate) fn generate_install_xml(config: &InstallConfig) -> Result<NamedTempFile> {
    todo!("Task 1 RED: 待实现 generate_install_xml")
}

/// 对 XML 属性值进行字符转义，防止路径中含 & < > " ' 等字符。
fn xml_escape(s: &str) -> String {
    todo!("Task 1 RED: 待实现 xml_escape")
}

fn run_silent_install_bin(extract_dir: &Path, xml_path: &Path) -> Result<()> {
    todo!("Task 1 RED: 待实现 run_silent_install_bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xml_contains_all_required_tags() {
        let config = InstallConfig::default();
        let xml_file = generate_install_xml(&config).unwrap();
        let content = std::fs::read_to_string(xml_file.path()).unwrap();
        assert!(content.contains("<INSTALL_PATH>"), "缺少 INSTALL_PATH 标签");
        assert!(content.contains("<PATH>"), "缺少 PATH 标签");
        assert!(content.contains("<INSTANCE_NAME>"), "缺少 INSTANCE_NAME 标签");
        assert!(content.contains("<PORT_NUM>"), "缺少 PORT_NUM 标签");
        assert!(content.contains("<PAGE_SIZE>"), "缺少 PAGE_SIZE 标签");
        assert!(content.contains("<CHARSET>"), "缺少 CHARSET 标签");
        assert!(content.contains("<CASE_SENSITIVE>"), "缺少 CASE_SENSITIVE 标签");
        assert!(content.contains("<EXTENT_SIZE>"), "缺少 EXTENT_SIZE 标签");
        assert!(
            content.contains("<CREATE_DB_SERVICE>N</CREATE_DB_SERVICE>"),
            "CREATE_DB_SERVICE 必须为 N"
        );
        assert!(
            content.contains("<STARTUP_DB_SERVICE>N</STARTUP_DB_SERVICE>"),
            "STARTUP_DB_SERVICE 必须为 N"
        );
    }

    #[test]
    fn test_xml_escapes_special_chars() {
        let config = InstallConfig {
            install_path: "/opt/dm&db<test>".to_string(),
            ..Default::default()
        };
        let xml_file = generate_install_xml(&config).unwrap();
        let content = std::fs::read_to_string(xml_file.path()).unwrap();
        assert!(content.contains("&amp;"), "& 必须转义为 &amp;");
        assert!(content.contains("&lt;"), "< 必须转义为 &lt;");
        assert!(content.contains("&gt;"), "> 必须转义为 &gt;");
        assert!(!content.contains("/opt/dm&db"), "原始 & 不应出现在 XML 中");
    }

    #[test]
    fn test_xml_case_sensitive_y_n() {
        let config_sensitive = InstallConfig {
            case_sensitive: true,
            ..Default::default()
        };
        let xml_file = generate_install_xml(&config_sensitive).unwrap();
        let content = std::fs::read_to_string(xml_file.path()).unwrap();
        assert!(
            content.contains("<CASE_SENSITIVE>Y</CASE_SENSITIVE>"),
            "case_sensitive=true 应生成 Y"
        );

        let config_insensitive = InstallConfig {
            case_sensitive: false,
            ..Default::default()
        };
        let xml_file2 = generate_install_xml(&config_insensitive).unwrap();
        let content2 = std::fs::read_to_string(xml_file2.path()).unwrap();
        assert!(
            content2.contains("<CASE_SENSITIVE>N</CASE_SENSITIVE>"),
            "case_sensitive=false 应生成 N"
        );
    }

    #[test]
    fn test_xml_create_db_service_is_n() {
        let config = InstallConfig::default();
        let xml_file = generate_install_xml(&config).unwrap();
        let content = std::fs::read_to_string(xml_file.path()).unwrap();
        assert!(
            content.contains("<CREATE_DB_SERVICE>N</CREATE_DB_SERVICE>"),
            "CREATE_DB_SERVICE 永远为 N"
        );
        assert!(
            !content.contains("<CREATE_DB_SERVICE>Y</CREATE_DB_SERVICE>"),
            "禁止 CREATE_DB_SERVICE=Y"
        );
    }
}
