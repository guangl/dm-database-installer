use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub mod ssh;

/// 约定通用配置文件名，安装时从当前目录自动读取。
pub const CONFIG_FILE: &str = "config.toml";

/// 安装包来源（三选一）。
#[derive(Debug, Clone)]
pub enum InstallerSource {
    /// 自动从内嵌 versions.txt 检测平台并下载（仅单机/单机 SSH 支持）
    Auto,
    /// 用户提供的本地文件路径
    LocalFile(PathBuf),
    /// 用户提供的自定义下载链接
    Url(String),
}

/// 通用配置（config.toml）：安装包来源。
/// 日志配置通过 load_log_config() 单独提前读取，SSH 凭证在各特有配置文件中单独配置。
#[derive(Debug)]
pub struct CommonConfig {
    pub installer: InstallerSource,
}

/// TOML 反序列化代理：接收原始字段，转换时校验互斥约束。
#[derive(Deserialize)]
struct CommonConfigRaw {
    #[serde(rename = "type")]
    _install_type: String,
    #[serde(default)]
    installer_package: Option<PathBuf>,
    #[serde(default)]
    installer_url: Option<String>,
}

impl TryFrom<CommonConfigRaw> for CommonConfig {
    type Error = anyhow::Error;
    fn try_from(raw: CommonConfigRaw) -> Result<Self> {
        let installer = match (raw.installer_package, raw.installer_url) {
            (Some(_), Some(_)) => {
                bail!("installer_package 和 installer_url 不能同时设置，请二选一")
            }
            (Some(path), None) => InstallerSource::LocalFile(path),
            (None, Some(url)) => InstallerSource::Url(url),
            (None, None) => InstallerSource::Auto,
        };
        Ok(CommonConfig { installer })
    }
}

/// 加载后的完整配置：通用配置 + 单机特有配置。
pub struct LoadedConfig {
    pub common: CommonConfig,
    pub specific: InstallConfig,
}

/// 从当前目录的 config.toml + standalone.toml 加载并验证配置。
pub fn load_config() -> Result<LoadedConfig> {
    load_config_from(Path::new(CONFIG_FILE))
}

/// 从指定 config.toml 路径加载（validate 子命令使用）。
pub fn load_config_from(common_path: &Path) -> Result<LoadedConfig> {
    let common = load_common_config(common_path)?;
    let dir = common_path.parent().unwrap_or(Path::new("."));
    let specific_path = dir.join("standalone.toml");
    let specific = load_standalone_specific(&specific_path)?;
    Ok(LoadedConfig { common, specific })
}

pub(super) fn load_common_config(path: &Path) -> Result<CommonConfig> {
    if !path.exists() {
        bail!(
            "未找到配置文件 {}\n请先运行 dm-installer init standalone",
            path.display()
        );
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取配置文件: {}", path.display()))?;
    let raw = toml::from_str::<CommonConfigRaw>(&content)
        .with_context(|| format!("配置文件解析失败: {}", path.display()))?;
    CommonConfig::try_from(raw).with_context(|| format!("配置文件无效: {}", path.display()))
}

pub(super) fn load_standalone_specific(path: &Path) -> Result<InstallConfig> {
    if !path.exists() {
        bail!("未找到单机特有配置文件 {}", path.display());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取单机配置文件: {}", path.display()))?;
    let file_cfg = toml::from_str::<InstallConfigFile>(&content)
        .with_context(|| format!("单机配置文件解析失败: {}", path.display()))?;
    let cfg = InstallConfig::from(file_cfg);
    validate_install_config(&cfg)?;
    Ok(cfg)
}

/// 归档配置，单机和集群共用。
/// 单机：仅本地归档；集群：本地归档 + 实时归档（REALTIME 段由集群层额外拼接）。
#[derive(Debug, Deserialize, Clone)]
pub struct ArchiveConfig {
    /// 归档目录，不填则默认为 {data_path}/arch
    #[serde(default)]
    pub arch_path: Option<String>,
    /// 单归档文件大小（MB），默认 128
    #[serde(default = "default_arch_file_size")]
    pub file_size: u32,
    /// 归档空间上限（MB），0 = 无限
    #[serde(default)]
    pub space_limit: u32,
    /// 归档失败时是否挂起数据库，默认 false
    #[serde(default)]
    pub hang_flag: bool,
    /// 是否压缩归档文件，默认 false
    #[serde(default)]
    pub compressed: bool,
}

fn default_arch_file_size() -> u32 {
    128
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        Self {
            arch_path: None,
            file_size: 128,
            space_limit: 0,
            hang_flag: false,
            compressed: false,
        }
    }
}

/// 解析归档目录：优先取配置值，否则用 `{data_path}/arch`。
pub(crate) fn resolve_arch_path(arch: &ArchiveConfig, data_path: &str) -> String {
    arch.arch_path
        .clone()
        .unwrap_or_else(|| format!("{}/arch", data_path))
}

/// 生成 dmarch.ini 的 LOCAL 段（单机和集群共用）。
pub(crate) fn format_local_arch_section(arch_path: &str, arch: &ArchiveConfig) -> String {
    format!(
        "[ARCHIVE_LOCAL1]\nARCH_TYPE = LOCAL\nARCH_DEST = {}\n\
         ARCH_FILE_SIZE = {}\nARCH_SPACE_LIMIT = {}\n\
         ARCH_HANG_FLAG = {}\nARCH_COMPRESSED = {}\n",
        arch_path, arch.file_size, arch.space_limit, arch.hang_flag as u8, arch.compressed as u8,
    )
}

/// 校验数据库初始化参数的值域约束（单机和集群共用）。
/// `ctx` 为错误前缀（单机传 `""`，集群传 `"dminit "`）。
pub(crate) fn validate_db_params(
    ctx: &str,
    port: u16,
    page_size: u8,
    charset: u8,
    extent_size: u8,
) -> anyhow::Result<()> {
    if port == 0 {
        anyhow::bail!("配置验证失败: {}port 无效: 0；有效范围为 1-65535", ctx);
    }
    if ![4u8, 8, 16, 32].contains(&page_size) {
        anyhow::bail!(
            "配置验证失败: {}page_size 无效: {}；有效值为 4/8/16/32",
            ctx,
            page_size
        );
    }
    if ![0u8, 1, 2].contains(&charset) {
        anyhow::bail!(
            "配置验证失败: {}charset 无效: {}；有效值 0=GB18030 1=UTF-8 2=EUC-KR",
            ctx,
            charset
        );
    }
    if ![16u8, 32].contains(&extent_size) {
        anyhow::bail!(
            "配置验证失败: {}extent_size 无效: {}；有效值为 16/32",
            ctx,
            extent_size
        );
    }
    Ok(())
}

/// 单机安装特有配置（运行时扁平结构，从 [install] + [instance] + [archive] + [ssh_target] 四组解析而来）。
#[derive(Debug)]
pub struct InstallConfig {
    pub install_path: String,
    pub data_path: String,
    pub instance_name: String,
    pub port: u16,
    pub ap_port: u16,
    pub page_size: u8,
    pub charset: u8,
    pub case_sensitive: bool,
    pub extent_size: u8,
    pub archive: ArchiveConfig,
    pub ssh_target: Option<ssh::SshTarget>,
}

// ── 私有代理结构体：对应 standalone.toml 的三个 TOML section ────────────────

#[derive(Deserialize)]
struct InstallSection {
    #[serde(default = "default_install_path")]
    install_path: String,
    #[serde(default = "default_data_path")]
    data_path: String,
}

impl Default for InstallSection {
    fn default() -> Self {
        Self {
            install_path: default_install_path(),
            data_path: default_data_path(),
        }
    }
}

#[derive(Deserialize)]
struct InstanceSection {
    #[serde(default = "default_instance_name")]
    instance_name: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_ap_port")]
    ap_port: u16,
    #[serde(default = "default_page_size")]
    page_size: u8,
    #[serde(default = "default_charset")]
    charset: u8,
    #[serde(default = "default_case_sensitive")]
    case_sensitive: bool,
    #[serde(default = "default_extent_size")]
    extent_size: u8,
}

impl Default for InstanceSection {
    fn default() -> Self {
        Self {
            instance_name: default_instance_name(),
            port: default_port(),
            ap_port: default_ap_port(),
            page_size: default_page_size(),
            charset: default_charset(),
            case_sensitive: default_case_sensitive(),
            extent_size: default_extent_size(),
        }
    }
}

#[derive(Deserialize)]
struct InstallConfigFile {
    #[serde(default)]
    install: InstallSection,
    #[serde(default)]
    instance: InstanceSection,
    #[serde(default)]
    archive: ArchiveConfig,
    ssh_target: Option<ssh::SshTarget>,
}

impl From<InstallConfigFile> for InstallConfig {
    fn from(f: InstallConfigFile) -> Self {
        Self {
            install_path: f.install.install_path,
            data_path: f.install.data_path,
            instance_name: f.instance.instance_name,
            port: f.instance.port,
            ap_port: f.instance.ap_port,
            page_size: f.instance.page_size,
            charset: f.instance.charset,
            case_sensitive: f.instance.case_sensitive,
            extent_size: f.instance.extent_size,
            archive: f.archive,
            ssh_target: f.ssh_target,
        }
    }
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
fn default_ap_port() -> u16 {
    4236
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
            ap_port: default_ap_port(),
            page_size: default_page_size(),
            charset: default_charset(),
            case_sensitive: default_case_sensitive(),
            extent_size: default_extent_size(),
            archive: ArchiveConfig::default(),
            ssh_target: None,
        }
    }
}

/// 验证 InstallConfig 字段语义合法性（枚举值域、范围约束）。
pub fn validate_install_config(cfg: &InstallConfig) -> Result<()> {
    validate_db_params("", cfg.port, cfg.page_size, cfg.charset, cfg.extent_size)?;
    if cfg.ap_port == 0 {
        bail!("配置验证失败: ap_port 无效: 0；有效范围为 1-65535");
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
        let cfg = InstallConfig {
            page_size: 12,
            ..Default::default()
        };
        let err = validate_install_config(&cfg).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("page_size 无效: 12"),
            "应含 'page_size 无效: 12'，实际: {msg}"
        );
        assert!(
            msg.contains("有效值为 4/8/16/32"),
            "应含 '有效值为 4/8/16/32'，实际: {msg}"
        );
    }

    #[test]
    fn test_validate_install_config_rejects_invalid_charset() {
        let cfg = InstallConfig {
            charset: 9,
            ..Default::default()
        };
        let err = validate_install_config(&cfg).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("charset 无效: 9"),
            "应含 'charset 无效: 9'，实际: {msg}"
        );
    }

    #[test]
    fn test_validate_install_config_rejects_invalid_extent_size() {
        let cfg = InstallConfig {
            extent_size: 8,
            ..Default::default()
        };
        let err = validate_install_config(&cfg).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("extent_size 无效: 8"),
            "应含 'extent_size 无效: 8'，实际: {msg}"
        );
    }

    #[test]
    fn test_validate_install_config_rejects_port_zero() {
        let cfg = InstallConfig {
            port: 0,
            ..Default::default()
        };
        let err = validate_install_config(&cfg).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("port 无效: 0"),
            "应含 'port 无效: 0'，实际: {msg}"
        );
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
    fn test_load_standalone_specific_valid() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "[instance]\nport = 5237\npage_size = 16").unwrap();
        let cfg = load_standalone_specific(file.path()).expect("应返回 Ok(InstallConfig)");
        assert_eq!(cfg.port, 5237);
        assert_eq!(cfg.page_size, 16);
    }

    #[test]
    fn test_load_standalone_specific_rejects_semantic_invalid() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "[instance]\npage_size = 12").unwrap();
        let err = load_standalone_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("page_size 无效: 12"),
            "应含 'page_size 无效: 12'，实际: {msg}"
        );
    }

    #[test]
    fn test_load_standalone_specific_missing_file_fails() {
        let err = load_standalone_specific(Path::new("/nonexistent/standalone.toml")).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("未找到单机特有配置文件"),
            "应含文件缺失提示，实际: {msg}"
        );
    }

    #[test]
    fn test_load_standalone_specific_syntax_error_fails() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "[instance]\nport = \"not_a_number\"").unwrap();
        let err = load_standalone_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("单机配置文件解析失败"),
            "应含 '单机配置文件解析失败'，实际: {msg}"
        );
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
