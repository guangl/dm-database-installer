use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

pub use crate::config::ssh::SshCredentials;
pub use crate::config::ArchiveConfig;

/// 节点角色：主节点、备节点或监控节点。
#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    Primary,
    Standby,
    /// 专用 dmmonitor 节点；不配置时 dmmonitor 运行在备节点上
    Monitor,
}

/// MAL 链路配置，对应 dmmal.ini 全局参数。
#[derive(Debug, Deserialize, Clone)]
pub struct MalConfig {
    /// MAL 心跳检测间隔（秒），默认 5
    #[serde(default = "default_mal_check_interval")]
    pub check_interval: u32,
    /// MAL 连接失败重试间隔（秒），默认 5
    #[serde(default = "default_mal_conn_fail_interval")]
    pub conn_fail_interval: u32,
    /// MAL 单实例发送缓冲区大小（MB），默认 100
    #[serde(default = "default_mal_buf_size")]
    pub buf_size: u32,
    /// MAL 系统级总发送缓冲区大小（MB），默认 512
    #[serde(default = "default_mal_sys_buf_size")]
    pub sys_buf_size: u32,
    /// MAL 数据压缩级别（0=不压缩），默认 0
    #[serde(default)]
    pub compress_level: u8,
}

fn default_mal_check_interval() -> u32 { 5 }
fn default_mal_conn_fail_interval() -> u32 { 5 }
fn default_mal_buf_size() -> u32 { 100 }
fn default_mal_sys_buf_size() -> u32 { 512 }

impl Default for MalConfig {
    fn default() -> Self {
        Self { check_interval: 5, conn_fail_interval: 5, buf_size: 100, sys_buf_size: 512, compress_level: 0 }
    }
}

/// dmwatcher 守护模式。
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum DwMode {
    Auto,
    Manual,
}

/// 守护进程配置，对应 dmwatcher.ini 全部可调参数。
#[derive(Debug, Deserialize, Clone)]
pub struct WatcherConfig {
    /// 守护模式：AUTO（自动故障切换）或 MANUAL（手动），默认 AUTO
    #[serde(default = "default_dw_mode")]
    pub dw_mode: DwMode,
    /// 守护错误判定时间（秒），默认 10
    #[serde(default = "default_dw_error_time")]
    pub dw_error_time: u32,
    /// 实例恢复等待时间（秒），默认 60
    #[serde(default = "default_inst_recover_time")]
    pub inst_recover_time: u32,
    /// 实例错误判定时间（秒），默认 10
    #[serde(default = "default_inst_error_time")]
    pub inst_error_time: u32,
    /// 实例故障后是否自动重启（1=是，0=否），默认 1
    #[serde(default = "default_inst_auto_restart")]
    pub inst_auto_restart: u8,
    /// redo 日志发送阈值（秒），0 表示不限制，默认 0
    #[serde(default)]
    pub rlog_send_threshold: u32,
    /// redo 日志应用阈值（秒），0 表示不限制，默认 0
    #[serde(default)]
    pub rlog_apply_threshold: u32,
    /// 自定义实例启动命令；不填则默认 {install_path}/bin/dmserver
    #[serde(default)]
    pub inst_startup_cmd: Option<String>,
}

fn default_dw_mode() -> DwMode { DwMode::Auto }
fn default_dw_error_time() -> u32 { 10 }
fn default_inst_recover_time() -> u32 { 60 }
fn default_inst_error_time() -> u32 { 10 }
fn default_inst_auto_restart() -> u8 { 1 }

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            dw_mode: DwMode::Auto,
            dw_error_time: 10,
            inst_recover_time: 60,
            inst_error_time: 10,
            inst_auto_restart: 1,
            rlog_send_threshold: 0,
            rlog_apply_threshold: 0,
            inst_startup_cmd: None,
        }
    }
}

/// SQL 日志配置，对应 sqllog.ini [SLOG_ALL] 段。
#[derive(Debug, Deserialize, Clone)]
pub struct SqlLogConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_sqllog_file_size")]
    pub file_size: u32,
    #[serde(default = "default_sqllog_file_num")]
    pub file_num: u32,
    #[serde(default)]
    pub min_exec_time: u32,
}

fn default_sqllog_file_size() -> u32 { 64 }
fn default_sqllog_file_num() -> u32 { 128 }

impl Default for SqlLogConfig {
    fn default() -> Self {
        Self { enabled: false, file_size: 64, file_num: 128, min_exec_time: 0 }
    }
}

/// dm.ini 集群追加参数配置，对应 [dm_ini] 段。
#[derive(Debug, Deserialize, Clone)]
pub struct DmIniConfig {
    #[serde(default = "default_enable_offline_ts")]
    pub enable_offline_ts: u8,
}

fn default_enable_offline_ts() -> u8 { 2 }

impl Default for DmIniConfig {
    fn default() -> Self { Self { enable_offline_ts: 2 } }
}

/// dminit 初始化参数，对应 `dminit` 命令行参数。
/// 集群级统一设置，所有节点相同；实例名 (instance_name) 在各节点上单独指定。
#[derive(Debug, Deserialize, Clone)]
pub struct DminitConfig {
    #[serde(default = "default_install_path")]
    pub install_path: String,
    #[serde(default = "default_data_path")]
    pub data_path: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_page_size")]
    pub page_size: u8,
    #[serde(default = "default_charset")]
    pub charset: u8,
    #[serde(default = "default_case_sensitive")]
    pub case_sensitive: bool,
    #[serde(default = "default_extent_size")]
    pub extent_size: u8,
    /// SYSDBA 用户密码，用于 disql 连接，默认 "SYSDBA"
    #[serde(default = "default_sysdba_password")]
    pub sysdba_password: String,
}

fn default_install_path() -> String { "/opt/dmdbms".to_string() }
fn default_data_path() -> String { "/opt/dmdbms/data".to_string() }
fn default_port() -> u16 { 5236 }
fn default_page_size() -> u8 { 8 }
fn default_charset() -> u8 { 0 }
fn default_case_sensitive() -> bool { true }
fn default_extent_size() -> u8 { 16 }
fn default_sysdba_password() -> String { "SYSDBA".to_string() }

impl Default for DminitConfig {
    fn default() -> Self {
        Self {
            install_path: default_install_path(),
            data_path: default_data_path(),
            port: default_port(),
            page_size: default_page_size(),
            charset: default_charset(),
            case_sensitive: default_case_sensitive(),
            extent_size: default_extent_size(),
            sysdba_password: default_sysdba_password(),
        }
    }
}

/// 单节点配置。
///
/// - 实例名 (instance_name) 是唯一的节点级字段，其他 dminit 参数在集群级 [dminit] 统一配置
/// - 连接参数（role / host / MAL 端口 / ssh）保留在顶层 [[nodes]]
#[derive(Debug, Deserialize, Clone)]
pub struct NodeConfig {
    pub role: NodeRole,
    pub host: String,
    /// 节点实例名（唯一，如 DMSVR01 / DMSVR02）
    pub instance_name: String,
    #[serde(default = "default_mal_port")]
    pub mal_port: u16,
    #[serde(default = "default_dw_port")]
    pub dw_port: u16,
    #[serde(default = "default_inst_dw_port")]
    pub inst_dw_port: u16,
    /// 读写分离模式下备节点标记为只读
    #[serde(default)]
    pub read_only: bool,
    pub ssh: SshCredentials,
}

fn default_mal_port() -> u16 { 5237 }
fn default_dw_port() -> u16 { 5238 }
fn default_inst_dw_port() -> u16 { 5239 }

/// DSC 共享存储磁盘配置，对应 dsc.toml 中的 [dsc_storage] 段。
///
/// 四个磁盘路径均为块设备路径，必须互不相同。
#[derive(Debug, Deserialize, Clone)]
pub struct DscStorageConfig {
    /// DCR 磁盘路径（块设备），如 /dev/raw/raw1
    pub dcr_disk: String,
    /// 表决磁盘路径（块设备），如 /dev/raw/raw2
    pub vote_disk: String,
    /// ASM 日志磁盘路径，如 /dev/raw/raw3（DMLOG 磁盘组）
    pub log_disk: String,
    /// ASM 数据磁盘路径，如 /dev/raw/raw4（DMDATA 磁盘组）
    pub data_disk: String,
}

impl Default for DscStorageConfig {
    fn default() -> Self {
        Self {
            dcr_disk: "/dev/raw/raw1".to_string(),
            vote_disk: "/dev/raw/raw2".to_string(),
            log_disk: "/dev/raw/raw3".to_string(),
            data_disk: "/dev/raw/raw4".to_string(),
        }
    }
}

/// 集群特有配置，对应 dw.toml / rws.toml / dsc.toml / dpc.toml。
#[derive(Debug, Deserialize)]
pub struct ClusterSpecificConfig {
    /// 守护系统全局唯一标识，守护系统内必须唯一，无默认值——必须显式配置
    pub oguid: u32,
    /// 节点列表
    #[serde(default)]
    pub nodes: Vec<NodeConfig>,
    /// DSC 专用：新版块设备磁盘配置（[dsc_storage] 段）
    pub dsc_storage: Option<DscStorageConfig>,
    /// DSC 旧版共享存储路径（已废弃，请迁移到 [dsc_storage]）
    /// 保留此字段以防 TOML 解析旧配置时报 unknown field 错误
    #[allow(dead_code)]
    pub shared_storage: Option<String>,
    /// dminit 初始化参数（集群级统一）
    #[serde(default)]
    pub dminit: DminitConfig,
    /// dm.ini 集群追加参数
    #[serde(default)]
    pub dm_ini: DmIniConfig,
    /// 归档配置（dmarch.ini 参数）
    #[serde(default)]
    pub archive: ArchiveConfig,
    /// MAL 链路配置（dmmal.ini 全局参数）
    #[serde(default)]
    pub mal: MalConfig,
    /// 守护进程配置（dmwatcher.ini 参数）
    #[serde(default)]
    pub watcher: WatcherConfig,
    /// SQL 日志配置
    #[serde(default)]
    pub sqllog: SqlLogConfig,
}

/// 从文件加载并验证集群特有配置。
pub fn load_cluster_specific(path: &Path, install_type: crate::config::InstallType) -> Result<ClusterSpecificConfig> {
    if !path.exists() {
        bail!("未找到集群特有配置文件 {}", path.display());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取集群特有配置文件: {}", path.display()))?;
    let cfg = toml::from_str::<ClusterSpecificConfig>(&content)
        .with_context(|| format!("集群特有配置文件解析失败: {}", path.display()))?;
    validate_cluster_specific_config(install_type, &cfg)?;
    Ok(cfg)
}

/// 验证集群特有配置的语义合法性，按集群类型分派。
pub fn validate_cluster_specific_config(
    install_type: crate::config::InstallType,
    cfg: &ClusterSpecificConfig,
) -> Result<()> {
    use crate::config::InstallType::*;
    match install_type {
        Dw => validate_primary_standby(cfg),
        Rws => validate_rws(cfg),
        Dsc => validate_dsc(cfg),
        Dpc => validate_primary_standby(cfg),
        Standalone => unreachable!("standalone 不使用集群配置"),
    }
}

fn validate_primary_standby(cfg: &ClusterSpecificConfig) -> Result<()> {
    check_nodes_not_empty(cfg)?;
    check_role_uniqueness(cfg)?;
    check_oguid_range(cfg)?;
    validate_dminit_config(&cfg.dminit)?;
    check_node_fields(cfg)?;
    check_instance_name_uniqueness(cfg)
}

fn validate_rws(cfg: &ClusterSpecificConfig) -> Result<()> {
    check_nodes_not_empty(cfg)?;
    check_role_uniqueness(cfg)?;
    check_oguid_range(cfg)?;
    validate_dminit_config(&cfg.dminit)?;
    check_node_fields(cfg)?;
    check_instance_name_uniqueness(cfg)?;
    let has_readonly_standby = cfg.nodes.iter().any(|n| n.role == NodeRole::Standby && n.read_only);
    if !has_readonly_standby {
        bail!("配置验证失败: 读写分离模式要求至少一个备节点设置 read_only = true");
    }
    Ok(())
}

fn validate_dsc(cfg: &ClusterSpecificConfig) -> Result<()> {
    check_nodes_not_empty(cfg)?;
    check_role_uniqueness(cfg)?;
    check_oguid_range(cfg)?;
    validate_dminit_config(&cfg.dminit)?;
    check_node_fields(cfg)?;
    check_instance_name_uniqueness(cfg)?;
    // DSC 不支持 Monitor 角色节点（部署逻辑中没有 dmwatcher/dmmonitor 处理）
    let has_monitor = cfg.nodes.iter().any(|n| n.role == NodeRole::Monitor);
    if has_monitor {
        bail!("配置验证失败: DSC 集群不支持 monitor 角色节点，请仅配置 primary/standby");
    }
    // DSC 集群至少需要 2 个节点（1 primary + 1 standby）
    let non_monitor_count = cfg.nodes.iter()
        .filter(|n| n.role != NodeRole::Monitor)
        .count();
    if non_monitor_count < 2 {
        bail!("配置验证失败: DSC 集群至少需要 2 个节点（1 primary + 1 standby）");
    }
    if cfg.dsc_storage.is_none() {
        bail!("配置验证失败: DSC 集群必须配置 [dsc_storage]（dcr_disk/vote_disk/log_disk/data_disk）");
    }
    let storage = cfg.dsc_storage.as_ref().unwrap();
    validate_dsc_storage(storage)
}

fn validate_dsc_storage(storage: &DscStorageConfig) -> Result<()> {
    let fields = [
        ("dcr_disk", &storage.dcr_disk),
        ("vote_disk", &storage.vote_disk),
        ("log_disk", &storage.log_disk),
        ("data_disk", &storage.data_disk),
    ];
    for (field_name, value) in &fields {
        if value.is_empty() {
            bail!("配置验证失败: DSC 磁盘路径不能为空: {}", field_name);
        }
    }
    let unique_paths: HashSet<&str> = fields.iter().map(|(_, v)| v.as_str()).collect();
    if unique_paths.len() < 4 {
        bail!("配置验证失败: DSC 磁盘路径必须互不相同（dcr_disk/vote_disk/log_disk/data_disk）");
    }
    Ok(())
}

fn check_nodes_not_empty(cfg: &ClusterSpecificConfig) -> Result<()> {
    if cfg.nodes.is_empty() {
        bail!("配置验证失败: 集群必须至少含一个节点");
    }
    Ok(())
}

fn check_role_uniqueness(cfg: &ClusterSpecificConfig) -> Result<()> {
    let primary_count = cfg.nodes.iter().filter(|n| n.role == NodeRole::Primary).count();
    if primary_count != 1 {
        bail!("配置验证失败: 必须恰好一个 primary 节点，当前有 {} 个", primary_count);
    }
    let monitor_count = cfg.nodes.iter().filter(|n| n.role == NodeRole::Monitor).count();
    if monitor_count > 1 {
        bail!("配置验证失败: 最多一个 monitor 节点，当前有 {} 个", monitor_count);
    }
    Ok(())
}

fn check_oguid_range(cfg: &ClusterSpecificConfig) -> Result<()> {
    if cfg.oguid > 2_147_483_647 {
        bail!("配置验证失败: oguid 越界: {}；有效范围 0-2147483647", cfg.oguid);
    }
    Ok(())
}

/// 校验集群级 dminit 参数（所有节点共用，只需验证一次）。
fn validate_dminit_config(dminit: &DminitConfig) -> Result<()> {
    crate::config::validate_db_params(
        "dminit ",
        dminit.port,
        dminit.page_size,
        dminit.charset,
        dminit.extent_size,
    )
}

fn check_node_fields(cfg: &ClusterSpecificConfig) -> Result<()> {
    for node in &cfg.nodes {
        validate_single_node(node, &cfg.dminit)?;
    }
    Ok(())
}

fn validate_single_node(node: &NodeConfig, dminit: &DminitConfig) -> Result<()> {
    if node.mal_port == dminit.port {
        bail!("配置验证失败: node[{}] mal_port 不能等于 dminit port: {}", node.host, dminit.port);
    }
    if node.ssh.identity_file.is_none() && node.ssh.password.is_none() {
        bail!("配置验证失败: node[{}] 至少提供 identity_file 或 password 之一", node.host);
    }
    Ok(())
}

fn check_instance_name_uniqueness(cfg: &ClusterSpecificConfig) -> Result<()> {
    let mut seen = HashSet::new();
    for node in &cfg.nodes {
        if !seen.insert(&node.instance_name) {
            bail!("配置验证失败: instance_name 重复: {}", node.instance_name);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InstallType;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_toml(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", content).unwrap();
        file
    }

    fn make_valid_toml() -> String {
        r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[dminit]
port = 5236
"#.to_string()
    }

    #[test]
    fn test_load_cluster_valid_two_nodes() {
        let file = write_toml(&make_valid_toml());
        let cfg = load_cluster_specific(file.path(), InstallType::Dw)
            .expect("应返回 Ok(ClusterSpecificConfig)");
        assert_eq!(cfg.nodes.len(), 2, "应有 2 个节点");
        assert_eq!(cfg.nodes[0].role, NodeRole::Primary);
        assert_eq!(cfg.nodes[1].role, NodeRole::Standby);
        assert_eq!(cfg.oguid, 453331);
    }

    #[test]
    fn test_load_cluster_valid_instance_names_different() {
        let file = write_toml(&make_valid_toml());
        let cfg = load_cluster_specific(file.path(), InstallType::Dw).unwrap();
        assert_eq!(cfg.nodes[0].instance_name, "DMSVR01");
        assert_eq!(cfg.nodes[1].instance_name, "DMSVR02");
    }

    #[test]
    fn test_load_cluster_valid_ssh_credentials() {
        use std::path::PathBuf;
        let file = write_toml(&make_valid_toml());
        let cfg = load_cluster_specific(file.path(), InstallType::Dw).unwrap();
        assert_eq!(cfg.nodes[0].ssh.user, "root");
        assert_eq!(cfg.nodes[0].ssh.identity_file, Some(PathBuf::from("~/.ssh/id_rsa")));
    }

    #[test]
    fn test_load_cluster_valid_default_ports() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let cfg = load_cluster_specific(file.path(), InstallType::Dw).unwrap();
        assert_eq!(cfg.nodes[0].mal_port, 5237, "mal_port 默认 5237");
        assert_eq!(cfg.nodes[0].dw_port, 5238, "dw_port 默认 5238");
        assert_eq!(cfg.nodes[0].inst_dw_port, 5239, "inst_dw_port 默认 5239");
        assert_eq!(cfg.dminit.port, 5236, "dminit.port 默认 5236");
    }

    #[test]
    fn test_validate_rejects_no_primary() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "standby"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_specific(file.path(), InstallType::Dw).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("必须恰好一个 primary 节点"), "应含 primary 错误，实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_port_conflict() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"
mal_port = 5236

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[dminit]
port = 5236
"#;
        let file = write_toml(toml);
        let err = load_cluster_specific(file.path(), InstallType::Dw).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("mal_port 不能等于 dminit port"), "应含端口冲突错误，实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_missing_ssh_credentials() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_specific(file.path(), InstallType::Dw).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("至少提供 identity_file 或 password 之一"), "应含 SSH 凭据错误，实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_oguid_overflow() {
        let toml = r#"
oguid = 2147483648

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_specific(file.path(), InstallType::Dw).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("oguid 越界"), "应含 oguid 越界错误，实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_invalid_page_size() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[dminit]
page_size = 12
"#;
        let file = write_toml(toml);
        let err = load_cluster_specific(file.path(), InstallType::Dw).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("page_size 无效"), "应含 page_size 错误，实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_duplicate_instance_name() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_specific(file.path(), InstallType::Dw).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("instance_name 重复"), "应含实例名重复错误，实际: {msg}");
    }

    #[test]
    fn test_rws_requires_readonly_standby() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_specific(file.path(), InstallType::Rws).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("read_only = true"), "应提示设置 read_only，实际: {msg}");
    }

    #[test]
    fn test_rws_accepts_readonly_standby() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
read_only = true
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        assert!(load_cluster_specific(file.path(), InstallType::Rws).is_ok(), "读写分离配置应合法");
    }

    #[test]
    fn test_dsc_storage_config_default_values() {
        let storage = DscStorageConfig::default();
        assert_eq!(storage.dcr_disk, "/dev/raw/raw1");
        assert_eq!(storage.vote_disk, "/dev/raw/raw2");
        assert_eq!(storage.log_disk, "/dev/raw/raw3");
        assert_eq!(storage.data_disk, "/dev/raw/raw4");
    }

    #[test]
    fn test_dsc_storage_config_deserializes() {
        let toml_str = r#"
dcr_disk = "/dev/sdb1"
vote_disk = "/dev/sdb2"
log_disk = "/dev/sdb3"
data_disk = "/dev/sdb4"
"#;
        let storage: DscStorageConfig = toml::from_str(toml_str).expect("反序列化应成功");
        assert_eq!(storage.dcr_disk, "/dev/sdb1");
        assert_eq!(storage.vote_disk, "/dev/sdb2");
        assert_eq!(storage.log_disk, "/dev/sdb3");
        assert_eq!(storage.data_disk, "/dev/sdb4");
    }

    #[test]
    fn test_dsc_requires_dsc_storage() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_specific(file.path(), InstallType::Dsc).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("dsc_storage"), "应提示配置 dsc_storage，实际: {msg}");
    }

    #[test]
    fn test_dsc_accepts_dsc_storage() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[dsc_storage]
dcr_disk = "/dev/raw/raw1"
vote_disk = "/dev/raw/raw2"
log_disk = "/dev/raw/raw3"
data_disk = "/dev/raw/raw4"
"#;
        let file = write_toml(toml);
        assert!(load_cluster_specific(file.path(), InstallType::Dsc).is_ok(), "DSC 配置应合法");
    }

    #[test]
    fn test_dsc_storage_disks_must_be_distinct() {
        let toml = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[dsc_storage]
dcr_disk = "/dev/raw/raw1"
vote_disk = "/dev/raw/raw1"
log_disk = "/dev/raw/raw3"
data_disk = "/dev/raw/raw4"
"#;
        let file = write_toml(toml);
        let err = load_cluster_specific(file.path(), InstallType::Dsc).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("互不相同") || msg.contains("distinct"), "应提示磁盘路径必须不同，实际: {msg}");
    }

    #[test]
    fn test_missing_oguid_fails() {
        let toml = r#"
[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_specific(file.path(), InstallType::Dw).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("oguid") || msg.contains("missing field"), "缺少 oguid 应报错，实际: {msg}");
    }
}
