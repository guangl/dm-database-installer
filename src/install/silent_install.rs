use anyhow::{Context, Result};
use std::{io::Write, path::Path, process::Command};
use tempfile::NamedTempFile;

use crate::config::InstallConfig;

/// 执行 DM 静默安装：生成 XML 响应文件 + 调用 DMInstall.bin -q。
pub fn run(config: &InstallConfig, extract_dir: &Path) -> Result<()> {
    let xml_file = generate_install_xml(config)?;
    run_silent_install_bin(extract_dir, xml_file.path())
}

/// 生成 DM 安装 XML 响应文件（含 XML 转义防注入）。
///
/// `<CREATE_DB_SERVICE>` 和 `<STARTUP_DB_SERVICE>` 固定为 N——服务注册
/// 由 `service.rs` 精确控制，禁止在 XML 中启用自动服务注册。
pub(crate) fn generate_install_xml(config: &InstallConfig) -> Result<NamedTempFile> {
    let install_path = xml_escape(&config.install_path);
    let data_path = xml_escape(&config.data_path);
    let instance_name = xml_escape(&config.instance_name);

    let xml = format!(
        r#"<?xml version="1.0"?>
<DATABASE>
  <LANGUAGE>zh</LANGUAGE>
  <TIME_ZONE>+08:00</TIME_ZONE>
  <INSTALL_TYPE>0</INSTALL_TYPE>
  <INSTALL_PATH>{install_path}</INSTALL_PATH>
  <INIT_DB>Y</INIT_DB>
  <DB_PARAMS>
    <PATH>{data_path}</PATH>
    <DB_NAME>DAMENG</DB_NAME>
    <INSTANCE_NAME>{instance_name}</INSTANCE_NAME>
    <PORT_NUM>{port}</PORT_NUM>
    <PAGE_SIZE>{page_size}</PAGE_SIZE>
    <CHARSET>{charset}</CHARSET>
    <CASE_SENSITIVE>{case_sensitive}</CASE_SENSITIVE>
    <EXTENT_SIZE>{extent_size}</EXTENT_SIZE>
    <CREATE_DB_SERVICE>N</CREATE_DB_SERVICE>
    <STARTUP_DB_SERVICE>N</STARTUP_DB_SERVICE>
  </DB_PARAMS>
</DATABASE>"#,
        port = config.port,
        page_size = config.page_size,
        charset = config.charset,
        case_sensitive = if config.case_sensitive { "Y" } else { "N" },
        extent_size = config.extent_size,
    );

    let mut file = NamedTempFile::new().context("创建 XML 临时文件失败")?;
    file.write_all(xml.as_bytes())?;
    Ok(file)
}

/// 调用 DMInstall.bin -q <xml_path> 执行静默安装。
fn run_silent_install_bin(extract_dir: &Path, xml_path: &Path) -> Result<()> {
    let dminstall = extract_dir.join("DMInstall.bin");

    // unix 下先设置可执行权限
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dminstall, std::fs::Permissions::from_mode(0o755))?;
    }

    let status = Command::new(&dminstall)
        .arg("-q")
        .arg(xml_path)
        .status()
        .with_context(|| format!("执行 DMInstall.bin 失败: {}", dminstall.display()))?;
    anyhow::ensure!(status.success(), "DMInstall.bin 返回非零退出码");
    Ok(())
}

/// 对 XML 文本内容进行基本字符转义（防 XML 注入）。
///
/// 顺序关键：`&` 必须最先替换，否则已转义的 `&amp;` 会被二次转义。
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
