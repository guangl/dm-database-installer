use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub mod cluster;
pub mod init;
pub mod ssh;
pub mod validate;

/// 约定通用配置文件名，安装时从当前目录自动读取。
pub const CONFIG_FILE: &str = "config.toml";

/// 安装类型，由 config.toml 的 type 字段确定，同时决定加载哪个特有配置文件。
#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum InstallType {
    Standalone,
    PrimaryStandby,
    Rws,
    Dsc,
    Dpc,
}

impl InstallType {
    /// 返回对应的特有配置文件名（与 config.toml 同目录）。
    pub fn specific_config_file(self) -> &'static str {
        match self {
            InstallType::Standalone => "standalone.toml",
            InstallType::PrimaryStandby => "primary-standby.toml",
            InstallType::Rws => "rws.toml",
            InstallType::Dsc => "dsc.toml",
            InstallType::Dpc => "dpc.toml",
        }
    }
}

/// 通用配置（config.toml）：安装类型、安装包来源、日志级别。
/// SSH 凭证在各特有配置文件中单独配置（standalone.toml / primary-standby.toml 等）。
#[derive(Debug, Deserialize)]
pub struct CommonConfig {
    /// 安装类型，决定特有配置文件的文件名
    #[serde(rename = "type")]
    pub install_type: InstallType,
    /// DM 安装包本地路径，不提供则自动下载（单机）或报错（集群）
    pub installer_package: Option<PathBuf>,
    /// 日志级别，默认 info
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_log_level() -> String { "info".to_string() }

/// 加载后的完整配置：通用配置 + 特有配置。
pub enum LoadedConfig {
    Standalone {
        common: CommonConfig,
        specific: InstallConfig,
    },
    Cluster {
        common: CommonConfig,
        specific: cluster::ClusterSpecificConfig,
        install_type: InstallType,
    },
}

/// 从当前目录的 config.toml + 对应特有文件加载并验证配置。
pub fn load_config() -> Result<LoadedConfig> {
    load_config_from(Path::new(CONFIG_FILE))
}

/// 从指定 config.toml 路径加载（validate 子命令使用）。
pub fn load_config_from(common_path: &Path) -> Result<LoadedConfig> {
    let common = load_common_config(common_path)?;
    let dir = common_path.parent().unwrap_or(Path::new("."));
    let specific_path = dir.join(common.install_type.specific_config_file());
    match common.install_type {
        InstallType::Standalone => {
            let specific = load_standalone_specific(&specific_path)?;
            Ok(LoadedConfig::Standalone { common, specific })
        }
        install_type => {
            let specific = cluster::load_cluster_specific(&specific_path, install_type)?;
            Ok(LoadedConfig::Cluster { common, specific, install_type })
        }
    }
}

pub(super) fn load_common_config(path: &Path) -> Result<CommonConfig> {
    if !path.exists() {
        bail!(
            "未找到配置文件 {}\n请先运行 dm-installer init standalone 或 dm-installer init cluster <type>",
            path.display()
        );
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取配置文件: {}", path.display()))?;
    toml::from_str::<CommonConfig>(&content)
        .with_context(|| format!("配置文件解析失败: {}", path.display()))
}

pub(super) fn load_standalone_specific(path: &Path) -> Result<InstallConfig> {
    if !path.exists() {
        bail!("未找到单机特有配置文件 {}", path.display());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取单机配置文件: {}", path.display()))?;
    let cfg = toml::from_str::<InstallConfig>(&content)
        .with_context(|| format!("单机配置文件解析失败: {}", path.display()))?;
    validate_install_config(&cfg)?;
    Ok(cfg)
}

/// 单机安装特有配置（standalone.toml）。密码不在此处，运行时由终端输入。
#[derive(Debug, Deserialize)]
pub struct InstallConfig {
    #[serde(default = "default_install_path")]
    pub install_path: String,
    #[serde(default = "default_data_path")]
    pub data_path: String,
    #[serde(default = "default_instance_name")]
    pub instance_name: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// 页大小（KB），可选 4/8/16/32，默认 32
    #[serde(default = "default_page_size")]
    pub page_size: u8,
    /// 字符集：0=GB18030, 1=UTF-8, 2=EUC-KR，默认 1（UTF-8）
    #[serde(default = "default_charset")]
    pub charset: u8,
    #[serde(default = "default_case_sensitive")]
    pub case_sensitive: bool,
    /// 区段大小（页数），可选 16/32，默认 32
    #[serde(default = "default_extent_size")]
    pub extent_size: u8,
    /// SSH 远程安装目标，不填则在本机安装
    pub ssh_target: Option<ssh::SshTarget>,
}

fn default_install_path() -> String { "/home/dmdba/dmdbms".to_string() }
fn default_data_path() -> String { "/home/dmdba/dmdbms/data".to_string() }
fn default_instance_name() -> String { "DMSERVER".to_string() }
fn default_port() -> u16 { 5236 }
fn default_page_size() -> u8 { 32 }
fn default_charset() -> u8 { 1 }
fn default_case_sensitive() -> bool { true }
fn default_extent_size() -> u8 { 32 }

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
            ssh_target: None,
        }
    }
}

/// 验证 InstallConfig 字段语义合法性（枚举值域、范围约束）。
pub fn validate_install_config(cfg: &InstallConfig) -> Result<()> {
    if ![4u8, 8, 16, 32].contains(&cfg.page_size) {
        bail!("配置验证失败: page_size 无效: {}；有效值为 4/8/16/32", cfg.page_size);
    }
    if ![0u8, 1, 2].contains(&cfg.charset) {
        bail!("配置验证失败: charset 无效: {}；有效值 0=GB18030 1=UTF-8 2=EUC-KR", cfg.charset);
    }
    if ![16u8, 32].contains(&cfg.extent_size) {
        bail!("配置验证失败: extent_size 无效: {}；有效值为 16/32", cfg.extent_size);
    }
    if cfg.port == 0 {
        bail!("配置验证失败: port 无效: 0；有效范围为 1-65535");
    }
    if let Some(target) = &cfg.ssh_target {
        if target.host.is_empty() {
            bail!("配置验证失败: ssh_target.host 不能为空");
        }
        if target.user.is_empty() {
            bail!("配置验证失败: ssh_target.user 不能为空");
        }
        if target.ssh_port == 0 {
            bail!("配置验证失败: ssh_target.ssh_port 无效: 0");
        }
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
                    let cfg = InstallConfig { page_size, charset, extent_size, port: 5236, ..Default::default() };
                    assert!(
                        validate_install_config(&cfg).is_ok(),
                        "page_size={page_size} charset={charset} extent_size={extent_size} 应合法"
                    );
                }
            }
        }
    }

    fn write_standalone(content: &str) -> (NamedTempFile, NamedTempFile) {
        let dir = tempfile::tempdir().unwrap();
        // common: config.toml
        let mut common = NamedTempFile::new_in(dir.path()).unwrap();
        writeln!(common, "type = \"standalone\"").unwrap();
        // specific: standalone.toml
        let mut specific = NamedTempFile::new_in(dir.path()).unwrap();
        writeln!(specific, "{}", content).unwrap();
        (common, specific)
    }

    #[test]
    fn test_load_standalone_specific_valid() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "port = 5237\npage_size = 16\n").unwrap();
        let cfg = load_standalone_specific(file.path()).expect("应返回 Ok(InstallConfig)");
        assert_eq!(cfg.port, 5237);
        assert_eq!(cfg.page_size, 16);
    }

    #[test]
    fn test_load_standalone_specific_rejects_semantic_invalid() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "page_size = 12\n").unwrap();
        let err = load_standalone_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("page_size 无效: 12"), "应含 'page_size 无效: 12'，实际: {msg}");
    }

    #[test]
    fn test_load_standalone_specific_missing_file_fails() {
        let err = load_standalone_specific(Path::new("/nonexistent/standalone.toml")).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("未找到单机特有配置文件"), "应含文件缺失提示，实际: {msg}");
    }

    #[test]
    fn test_load_standalone_specific_syntax_error_fails() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "port = \"not_a_number\"\n").unwrap();
        let err = load_standalone_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("单机配置文件解析失败"), "应含 '单机配置文件解析失败'，实际: {msg}");
    }

    #[test]
    fn test_install_config_defaults() {
        let cfg = InstallConfig::default();
        assert_eq!(cfg.install_path, "/home/dmdba/dmdbms");
        assert_eq!(cfg.data_path, "/home/dmdba/dmdbms/data");
        assert_eq!(cfg.instance_name, "DMSERVER");
        assert_eq!(cfg.port, 5236);
        assert_eq!(cfg.page_size, 32);
        assert_eq!(cfg.charset, 1);
        assert!(cfg.case_sensitive);
        assert_eq!(cfg.extent_size, 32);
    }
}
