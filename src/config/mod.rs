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
            "未找到配置文件 {}\n请先运行 dm_installer init standalone",
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
    /// 单归档文件大小（MB），默认 1024
    #[serde(default = "default_arch_file_size")]
    pub file_size: u32,
    /// 归档空间上限（MB），不填则默认为磁盘总容量的 20%；显式填 0 = 无限
    #[serde(default)]
    pub space_limit: Option<u32>,
}

fn default_arch_file_size() -> u32 {
    1024
}

/// 备份作业配置，单机和集群共用。
/// 默认策略：每 7 天（即每周六）执行一次全量备份，其余天执行一次增量备份，
/// 备份保留 `retain_days` 天，每天清理过期备份。
///
/// `full_backup_interval_days` 决定全量备份频率：
/// - `1`：每天只做全量备份，不创建增量备份作业
/// - `7`（默认）：与原版行为一致，全量固定在每周六，增量固定在周日至周五，按自然周对齐
/// - 其他值（如 `3`）：全量备份按 N 天间隔调度，增量备份按天调度；
///   由于达梦作业系统的间隔调度无法精确排除"恰好与全量同一天"，
///   两者重合的那天会同时执行全量和增量（增量备份内容会很少，不影响数据安全）
#[derive(Debug, Deserialize, Clone)]
pub struct BackupConfig {
    /// 数据库备份目录，必须配置（用于创建备份作业）
    #[serde(default)]
    pub backup_path: Option<String>,
    /// 备份保留天数，至少 15 天（按达梦官方建议的最小值）
    #[serde(default = "default_retain_days")]
    pub retain_days: u32,
    /// 全量备份间隔天数，至少 1 天，默认 7 天（每周六）
    #[serde(default = "default_full_backup_interval_days")]
    pub full_backup_interval_days: u32,
    /// 全量备份执行时间，格式 HH:MM:SS，默认 02:00
    #[serde(default = "default_full_backup_time")]
    pub full_backup_time: String,
    /// 增量备份执行时间，格式 HH:MM:SS，默认 02:00
    #[serde(default = "default_incr_backup_time")]
    pub incr_backup_time: String,
    /// 过期备份清理执行时间，格式 HH:MM:SS，默认每天 05:00
    #[serde(default = "default_clean_time")]
    pub clean_time: String,
}

fn default_retain_days() -> u32 {
    15
}

fn default_full_backup_interval_days() -> u32 {
    7
}

fn default_full_backup_time() -> String {
    "02:00:00".to_string()
}

fn default_incr_backup_time() -> String {
    "02:00:00".to_string()
}

fn default_clean_time() -> String {
    "05:00:00".to_string()
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            backup_path: None,
            retain_days: default_retain_days(),
            full_backup_interval_days: default_full_backup_interval_days(),
            full_backup_time: default_full_backup_time(),
            incr_backup_time: default_incr_backup_time(),
            clean_time: default_clean_time(),
        }
    }
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        Self {
            arch_path: None,
            file_size: default_arch_file_size(),
            space_limit: None,
        }
    }
}

/// 解析归档目录：优先取配置值，否则用 `{data_path}/arch`。
pub(crate) fn resolve_arch_path(arch: &ArchiveConfig, data_path: &str) -> String {
    arch.arch_path
        .clone()
        .unwrap_or_else(|| format!("{}/arch", data_path))
}

/// 校验 `HH:MM:SS` 格式的时间字符串（备份作业调度时间）。
fn validate_time_hhmmss(field: &str, value: &str) -> Result<()> {
    let parts: Vec<&str> = value.split(':').collect();
    let valid = parts.len() == 3
        && parts.iter().all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_digit()))
        && parts[0].parse::<u8>().is_ok_and(|h| h < 24)
        && parts[1].parse::<u8>().is_ok_and(|m| m < 60)
        && parts[2].parse::<u8>().is_ok_and(|s| s < 60);
    if !valid {
        bail!(
            "配置验证失败: {} 无效: \"{}\"；格式应为 HH:MM:SS（如 02:00:00）",
            field,
            value
        );
    }
    Ok(())
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
    pub page_size: u8,
    pub charset: u8,
    pub case_sensitive: bool,
    pub extent_size: u8,
    pub archive: ArchiveConfig,
    pub backup: BackupConfig,
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
    #[serde(default)]
    backup: BackupConfig,
    ssh_target: Option<ssh::SshTarget>,
}

impl From<InstallConfigFile> for InstallConfig {
    fn from(f: InstallConfigFile) -> Self {
        Self {
            install_path: f.install.install_path,
            data_path: f.install.data_path,
            instance_name: f.instance.instance_name,
            port: f.instance.port,
            page_size: f.instance.page_size,
            charset: f.instance.charset,
            case_sensitive: f.instance.case_sensitive,
            extent_size: f.instance.extent_size,
            archive: f.archive,
            backup: f.backup,
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
/// AP（应用）端口仅用于安装前端口占用预检查，不可配置，固定为达梦默认值。
pub(crate) const AP_PORT_PRECHECK: u16 = 4236;

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
            archive: ArchiveConfig::default(),
            backup: BackupConfig::default(),
            ssh_target: None,
        }
    }
}

/// 验证 InstallConfig 字段语义合法性（枚举值域、范围约束）。
pub fn validate_install_config(cfg: &InstallConfig) -> Result<()> {
    validate_db_params("", cfg.port, cfg.page_size, cfg.charset, cfg.extent_size)?;
    match cfg.backup.backup_path.as_deref() {
        None | Some("") => bail!(
            "配置验证失败: backup_path 未配置；请在 standalone.toml [backup] 段配置 backup_path（用于创建备份作业）"
        ),
        _ => {}
    }
    if cfg.backup.retain_days < 15 {
        bail!(
            "配置验证失败: backup.retain_days 无效: {}；至少保留 15 天",
            cfg.backup.retain_days
        );
    }
    if cfg.backup.full_backup_interval_days < 1 {
        bail!(
            "配置验证失败: backup.full_backup_interval_days 无效: {}；至少为 1 天",
            cfg.backup.full_backup_interval_days
        );
    }
    validate_time_hhmmss("backup.full_backup_time", &cfg.backup.full_backup_time)?;
    validate_time_hhmmss("backup.incr_backup_time", &cfg.backup.incr_backup_time)?;
    validate_time_hhmmss("backup.clean_time", &cfg.backup.clean_time)?;
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
                        backup: BackupConfig {
                            backup_path: Some("/data/dmbak".to_string()),
                            ..Default::default()
                        },
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
        writeln!(
            file,
            "[backup]\nbackup_path = \"/data/dmbak\"\n[instance]\nport = 5237\npage_size = 16"
        )
        .unwrap();
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
