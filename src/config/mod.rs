use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::Path;

pub mod cluster;
pub mod init;
pub mod ssh;
pub mod validate;

/// 安装配置，从 TOML 文件反序列化。密码不在此处，运行时由终端输入。
#[derive(Debug, Deserialize)]
pub struct InstallConfig {
    /// DM 安装根目录，默认 /home/dmdba/dmdbms
    #[serde(default = "default_install_path")]
    pub install_path: String,

    /// 数据文件目录，默认 /home/dmdba/dmdbms/data
    #[serde(default = "default_data_path")]
    pub data_path: String,

    /// 数据库实例名，默认 DMSERVER
    #[serde(default = "default_instance_name")]
    pub instance_name: String,

    /// 监听端口，默认 5236
    #[serde(default = "default_port")]
    pub port: u16,

    /// 页大小（KB），可选 4/8/16/32，默认 32
    #[serde(default = "default_page_size")]
    pub page_size: u8,

    /// 字符集：0=GB18030, 1=UTF-8, 2=EUC-KR，默认 1（UTF-8）
    #[serde(default = "default_charset")]
    pub charset: u8,

    /// 大小写敏感，默认 true
    #[serde(default = "default_case_sensitive")]
    pub case_sensitive: bool,

    /// 区段大小（页数），可选 16/32，默认 32
    #[serde(default = "default_extent_size")]
    pub extent_size: u8,
}

fn default_install_path() -> String {
    "/home/dmdba/dmdbms".to_string()
}
fn default_data_path() -> String {
    "/home/dmdba/dmdbms/data".to_string()
}
fn default_instance_name() -> String {
    "DMSERVER".to_string()
}
fn default_port() -> u16 {
    5236
}
fn default_page_size() -> u8 {
    32
}
fn default_charset() -> u8 {
    1
}
fn default_case_sensitive() -> bool {
    true
}
fn default_extent_size() -> u8 {
    32
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            install_path: default_install_path(),
            data_path: default_data_path(),
            instance_name: default_instance_name(),
            port: default_port(),
            page_size: default_page_size(),
            charset: default_charset(),
            case_sensitive: default_case_sensitive(),
            extent_size: default_extent_size(),
        }
    }
}

/// 从 TOML 文件加载配置并执行语义验证。
/// 三步链：读文件 → TOML 反序列化 → 语义验证。
pub fn load_and_validate(path: &Path) -> Result<InstallConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取配置文件: {}", path.display()))?;
    let cfg = toml::from_str::<InstallConfig>(&content).with_context(|| "配置文件解析失败")?;
    validate_install_config(&cfg)?;
    Ok(cfg)
}

/// 验证 InstallConfig 字段语义合法性（枚举值域、范围约束）。
pub fn validate_install_config(cfg: &InstallConfig) -> Result<()> {
    if ![4u8, 8, 16, 32].contains(&cfg.page_size) {
        bail!(
            "配置验证失败: page_size 无效: {}；有效值为 4/8/16/32",
            cfg.page_size
        );
    }
    if ![0u8, 1, 2].contains(&cfg.charset) {
        bail!(
            "配置验证失败: charset 无效: {}；有效值 0=GB18030 1=UTF-8 2=EUC-KR",
            cfg.charset
        );
    }
    if ![16u8, 32].contains(&cfg.extent_size) {
        bail!(
            "配置验证失败: extent_size 无效: {}；有效值为 16/32",
            cfg.extent_size
        );
    }
    if cfg.port == 0 {
        bail!("配置验证失败: port 无效: 0；有效范围为 1-65535");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_validate_install_config_rejects_invalid_page_size() {
        let cfg = InstallConfig { page_size: 12, ..Default::default() };
        let err = validate_install_config(&cfg).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("page_size 无效: 12"), "应含 'page_size 无效: 12'，实际: {msg}");
        assert!(msg.contains("有效值为 4/8/16/32"), "应含 '有效值为 4/8/16/32'，实际: {msg}");
    }

    #[test]
    fn test_validate_install_config_rejects_invalid_charset() {
        let cfg = InstallConfig { charset: 9, ..Default::default() };
        let err = validate_install_config(&cfg).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("charset 无效: 9"), "应含 'charset 无效: 9'，实际: {msg}");
    }

    #[test]
    fn test_validate_install_config_rejects_invalid_extent_size() {
        let cfg = InstallConfig { extent_size: 8, ..Default::default() };
        let err = validate_install_config(&cfg).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("extent_size 无效: 8"), "应含 'extent_size 无效: 8'，实际: {msg}");
    }

    #[test]
    fn test_validate_install_config_rejects_port_zero() {
        let cfg = InstallConfig { port: 0, ..Default::default() };
        let err = validate_install_config(&cfg).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("port 无效: 0"), "应含 'port 无效: 0'，实际: {msg}");
        assert!(msg.contains("1-65535"), "应含 '1-65535'，实际: {msg}");
    }

    #[test]
    fn test_validate_install_config_accepts_all_valid_combinations() {
        for &page_size in &[4u8, 8, 16, 32] {
            for &charset in &[0u8, 1, 2] {
                for &extent_size in &[16u8, 32] {
                    let cfg = InstallConfig {
                        page_size,
                        charset,
                        extent_size,
                        port: 5236,
                        ..Default::default()
                    };
                    assert!(
                        validate_install_config(&cfg).is_ok(),
                        "page_size={page_size} charset={charset} extent_size={extent_size} 应合法"
                    );
                }
            }
        }
    }

    #[test]
    fn test_load_and_validate_reads_tempfile_returns_config() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "port = 5237\npage_size = 16\n").unwrap();
        let cfg = load_and_validate(file.path()).expect("应返回 Ok(InstallConfig)");
        assert_eq!(cfg.port, 5237, "port 应为 5237");
        assert_eq!(cfg.page_size, 16, "page_size 应为 16");
    }

    #[test]
    fn test_load_and_validate_rejects_semantic_invalid_toml() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "page_size = 12\n").unwrap();
        let err = load_and_validate(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("page_size 无效: 12"),
            "应含 'page_size 无效: 12'，实际: {msg}"
        );
    }

    #[test]
    fn test_load_and_validate_missing_file_fails() {
        let err = load_and_validate(std::path::Path::new("/nonexistent/path/dm.toml")).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("无法读取配置文件"), "应含 '无法读取配置文件'，实际: {msg}");
    }

    #[test]
    fn test_load_and_validate_syntax_error_fails() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "port = \"not_a_number\"\n").unwrap();
        let err = load_and_validate(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("配置文件解析失败"), "应含 '配置文件解析失败'，实际: {msg}");
    }
}
