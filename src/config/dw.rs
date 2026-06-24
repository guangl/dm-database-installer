use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

use super::ssh::SshCredentials;
use super::{ArchiveConfig, BackupConfig, InstallConfig, validate_db_params};

/// 主备集群节点角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    Primary,
    Standby,
}

/// 守护切换模式：AUTO = 故障时自动切换主备；MANUAL（默认）= 需人工介入切换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum DwMode {
    Auto,
    #[default]
    Manual,
}

impl DwMode {
    pub fn as_str(self) -> &'static str {
        match self {
            DwMode::Auto => "AUTO",
            DwMode::Manual => "MANUAL",
        }
    }
}

/// 备库类型（dmarch.ini ARCH_TYPE），对应官方文档 §7.5/§7.6/§7.7 三种主备搭建方式。
/// 三者是独立的归档类型，并非"实时=同步"的两两对应关系：
/// - Realtime（默认）= 实时备库，ARCH_TYPE=REALTIME，本项目现有主备失败切换对（dmwatcher 管理）使用此类型；
/// - Sync = 同步备库，ARCH_TYPE=SYNC，主库等待该备库确认归档已落盘/恢复完成（ARCH_RECOVER_TIME 控制检测间隔）；
/// - Async = 异步备库，ARCH_TYPE=ASYNC，主库不等待确认，通过定时器（ARCH_TIMER_NAME）定期触发归档发送。
/// 仅对 standby 节点有意义；primary 节点该字段不参与渲染。
#[derive(Debug, Clone, Copy, PartialEq, Eq, std::hash::Hash, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StandbyMode {
    #[default]
    Realtime,
    Sync,
    Async,
}

impl StandbyMode {
    /// dmarch.ini 中对应的 ARCH_TYPE 取值。
    pub fn arch_type(self) -> &'static str {
        match self {
            StandbyMode::Realtime => "REALTIME",
            StandbyMode::Sync => "SYNC",
            StandbyMode::Async => "ASYNC",
        }
    }
}

/// 异步备库（ARCH_TYPE=ASYNC）的默认定时器名，引用 DM 内置系统定时器。
fn default_arch_timer_name() -> String {
    "RT_TIMER".to_string()
}

/// dmwatcher.ini 守护进程配置，对应 dw.toml 的 [watcher] 段。
/// 字段含义与默认值参考达梦官方文档 §5.4 dmwatcher.ini。
#[derive(Debug, Clone, Deserialize)]
pub struct WatcherConfig {
    // ── 基础 ──────────────────────────────────────────────────────────
    /// 切换模式：MANUAL（默认，人工介入）/ AUTO（故障自动切换）。
    #[serde(default)]
    pub dw_mode: DwMode,
    /// 守护进程故障确认时间（秒），默认 15，范围 3–1800。
    /// 守护进程连续检测到实例无响应超过此时长才判定为故障。
    #[serde(default = "default_dw_error_time")]
    pub dw_error_time: u32,
    /// 数据库实例故障确认时间（秒），默认 15，范围 3–1800。
    /// 与 dw_error_time 独立；数据库层面的无响应超时。
    #[serde(default = "default_inst_error_time")]
    pub inst_error_time: u32,
    /// 备库恢复检测间隔（秒），默认 60，范围 3–86400。
    /// 故障切换后，守护进程等待原主库重新接入的轮询间隔。
    #[serde(default = "default_inst_recover_time")]
    pub inst_recover_time: u32,

    // ── 重启策略 ──────────────────────────────────────────────────────
    /// 实例崩溃后是否自动重启，默认 0（否）。
    #[serde(default = "default_inst_auto_restart")]
    pub inst_auto_restart: u8,
    /// 最大连续自动重启次数，默认 0（不限制），范围 0–1024。
    /// inst_auto_restart=1 时有效；超过次数后守护进程停止重启。
    #[serde(default = "default_inst_restart_cnt")]
    pub inst_restart_cnt: u32,

    // ── 故障切换行为 ──────────────────────────────────────────────────
    /// MANUAL 模式下，无监视器时主库强制 Open 的超时（秒），默认 0（不超时等待）。
    /// 超过此时长仍未收到监视器指令则强制 Open，0 = 永久等待。
    #[serde(default = "default_dw_open_force_timeout")]
    pub dw_open_force_timeout: u32,
    /// 无监视器时是否允许主库强制 Open，默认 1（允许）。
    #[serde(default = "default_dw_failover_force")]
    pub dw_failover_force: u8,
    /// 断链重连策略，默认 1。0=不重连；1=重连后继续守护；2=重连后降为 OPEN 模式。
    #[serde(default = "default_dw_reconnect")]
    pub dw_reconnect: u8,

    // ── 监控阈值 ──────────────────────────────────────────────────────
    /// 实时归档发送延迟告警阈值（秒），默认 0（不告警）。
    #[serde(default = "default_rlog_send_threshold")]
    pub rlog_send_threshold: u32,
    /// 备库日志应用延迟告警阈值（秒），默认 0（不告警）。
    #[serde(default = "default_rlog_apply_threshold")]
    pub rlog_apply_threshold: u32,
    /// 是否检测主库对外服务 IP 可达性，默认 0（不检测）。
    #[serde(default = "default_inst_service_ip_check")]
    pub inst_service_ip_check: u8,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            dw_mode: DwMode::default(),
            dw_error_time: default_dw_error_time(),
            inst_error_time: default_inst_error_time(),
            inst_recover_time: default_inst_recover_time(),
            inst_auto_restart: default_inst_auto_restart(),
            inst_restart_cnt: default_inst_restart_cnt(),
            dw_open_force_timeout: default_dw_open_force_timeout(),
            dw_failover_force: default_dw_failover_force(),
            dw_reconnect: default_dw_reconnect(),
            rlog_send_threshold: default_rlog_send_threshold(),
            rlog_apply_threshold: default_rlog_apply_threshold(),
            inst_service_ip_check: default_inst_service_ip_check(),
        }
    }
}

/// 主备集群（dw.toml）单个节点配置。
#[derive(Debug, Clone)]
pub struct DwNode {
    pub role: NodeRole,
    pub host: String,
    pub instance_name: String,
    pub install_path: String,
    pub data_path: String,
    pub port: u16,
    pub mal_port: u16,
    pub dw_port: u16,
    pub inst_dw_port: u16,
    pub page_size: u8,
    pub charset: u8,
    pub case_sensitive: bool,
    pub extent_size: u8,
    /// 本节点归档目录，不填则默认为 {data_path}/arch。
    pub arch_path: Option<String>,
    /// 备库类型：Realtime（默认，REALTIME）/ Sync（SYNC）/ Async（ASYNC）。仅 standby 节点有意义。
    pub sync_mode: StandbyMode,
    /// 异步备库定时器名（ARCH_TIMER_NAME），仅 sync_mode=Async 时写入 dmarch.ini，默认 "RT_TIMER"。
    pub arch_timer_name: String,
    /// 备份作业配置：仅 primary 节点需要填写，standby 不需要（备份作业会由主库同步过去）。
    pub backup: Option<BackupConfig>,
    pub ssh: SshCredentials,
}

impl DwNode {
    /// 桥接到 `InstallConfig`，复用单机安装步骤函数（dminit/service/preflight 等）。
    /// 集群的归档走 dmarch.ini 文件而非 `archive` 模块的在线 SQL 路径，因此该字段留空
    /// 对集群安装无副作用；`backup` 仅 primary 有值，standby 传空配置（不会触发备份作业）。
    pub fn as_install_config(&self) -> InstallConfig {
        InstallConfig {
            install_path: self.install_path.clone(),
            data_path: self.data_path.clone(),
            instance_name: self.instance_name.clone(),
            port: self.port,
            page_size: self.page_size,
            charset: self.charset,
            case_sensitive: self.case_sensitive,
            extent_size: self.extent_size,
            archive: ArchiveConfig::default(),
            backup: self.backup.clone().unwrap_or_default(),
            ssh_target: None,
        }
    }

    /// 解析本节点归档目录：优先取配置值，否则用 `{data_path}/arch`。
    pub fn resolve_arch_path(&self) -> String {
        self.arch_path
            .clone()
            .unwrap_or_else(|| format!("{}/arch", self.data_path))
    }
}

/// dmmal.ini MAL 通信层全局参数，对应 dw.toml 的 [mal] 段。
/// 每节点的 MAL_INST_NAME / MAL_HOST / MAL_PORT 等由 [[nodes]] 自动推导，无需在此配置。
/// 字段含义参考达梦官方文档 §5.2 dmmal.ini。
#[derive(Debug, Clone, Deserialize)]
pub struct DwMalConfig {
    /// MAL 链路检测间隔（秒），默认 30，0=禁用检测，范围 0–1800。
    #[serde(default = "default_mal_check_interval")]
    pub mal_check_interval: u32,
    /// 判定 MAL 连接失败的时长阈值（秒），默认 10，范围 2–1800。
    #[serde(default = "default_mal_conn_fail_interval")]
    pub mal_conn_fail_interval: u32,
    /// 节点间登录超时（秒），默认 15，范围 3–1800。
    #[serde(default = "default_mal_login_timeout")]
    pub mal_login_timeout: u32,
    /// 单连接缓冲区上限（MB），默认 100，0=不限，范围 0–500000。
    #[serde(default = "default_mal_buf_size")]
    pub mal_buf_size: u32,
    /// 系统全局 MAL 内存上限（MB），默认 0（不限），范围 0–500000。
    #[serde(default = "default_mal_sys_buf_size")]
    pub mal_sys_buf_size: u32,
    /// 消息压缩级别，默认 0（不压缩）。0=无；1–9=lz 压缩；10=snappy。
    /// 注意：集群所有节点必须配置相同的压缩级别，否则无法建立 MAL 链路。
    #[serde(default = "default_mal_compress_level")]
    pub mal_compress_level: u8,
}

impl Default for DwMalConfig {
    fn default() -> Self {
        Self {
            mal_check_interval: default_mal_check_interval(),
            mal_conn_fail_interval: default_mal_conn_fail_interval(),
            mal_login_timeout: default_mal_login_timeout(),
            mal_buf_size: default_mal_buf_size(),
            mal_sys_buf_size: default_mal_sys_buf_size(),
            mal_compress_level: default_mal_compress_level(),
        }
    }
}

/// dmarch.ini 归档全局及本地归档参数，对应 dw.toml 的 [arch] 段。
/// 全局参数写在 dmarch.ini 文件顶部（节前），本地归档参数写在 [ARCHIVE_LOCAL*] 节内。
/// 字段含义参考达梦官方文档 §5.3 dmarch.ini。
#[derive(Debug, Clone, Deserialize)]
pub struct DwArchConfig {
    // ── 全局参数（节前） ───────────────────────────────────────────────
    /// 同步备库是否等待日志应用后再回应主库，默认 1（等待），0=不等待。
    #[serde(default = "default_arch_wait_apply")]
    pub arch_wait_apply: u8,
    /// 本地归档保留时长（分钟），超过后自动清理，默认 0（不自动清理）。
    #[serde(default = "default_arch_reserve_time")]
    pub arch_reserve_time: u32,
    /// 主库发送策略，默认 0。0=立即等待备库响应；1=先写本地再发送。
    #[serde(default = "default_arch_send_policy")]
    pub arch_send_policy: u8,
    /// 同步备库归档状态检测间隔（秒），默认 60，范围 1–86400。
    #[serde(default = "default_arch_recover_time")]
    pub arch_recover_time: u32,

    // ── 本地归档节参数（[ARCHIVE_LOCAL*]） ─────────────────────────────
    /// 本地归档单文件大小（MB），默认 1024，范围 64–2048。
    #[serde(default = "default_arch_file_size")]
    pub arch_file_size: u32,
    /// 本地归档空间上限（MB）：不填（None）= 自动取归档目录所在磁盘总容量的 20%，
    /// 探测失败时退回默认值 [`ARCH_SPACE_LIMIT_FALLBACK_MB`]（20GB）；
    /// 显式填 0 = 不限；填其他正整数 = 固定上限。
    #[serde(default)]
    pub arch_space_limit: Option<u32>,
}

/// 探测磁盘容量失败时的归档空间上限兜底默认值（MB），= 20GB。
pub const ARCH_SPACE_LIMIT_FALLBACK_MB: u32 = 20 * 1024;
/// 自动模式下取归档目录所在磁盘总容量的百分比。
pub const ARCH_SPACE_LIMIT_DISK_PERCENT: u64 = 20;

impl Default for DwArchConfig {
    fn default() -> Self {
        Self {
            arch_wait_apply: default_arch_wait_apply(),
            arch_reserve_time: default_arch_reserve_time(),
            arch_send_policy: default_arch_send_policy(),
            arch_recover_time: default_arch_recover_time(),
            arch_file_size: default_arch_file_size(),
            arch_space_limit: None,
        }
    }
}

/// dmmonitor.ini 监视器日志参数，对应 dw.toml 的 [monitor] 段。
/// 字段含义参考达梦官方文档 §5.5 dmmonitor.ini。
#[derive(Debug, Clone, Deserialize)]
pub struct DwMonitorConfig {
    /// 监视器日志输出目录，默认 "."（dmmonitor 启动时的当前目录）。
    #[serde(default = "default_mon_log_path")]
    pub mon_log_path: String,
    /// 日志刷新间隔（秒），默认 60，范围 1–600。
    #[serde(default = "default_mon_log_interval")]
    pub mon_log_interval: u32,
    /// 单个日志文件大小上限（MB），默认 32，范围 1–2048。
    #[serde(default = "default_mon_log_file_size")]
    pub mon_log_file_size: u32,
    /// 日志总空间上限（MB），默认 0（不限）。
    #[serde(default = "default_mon_log_space_limit")]
    pub mon_log_space_limit: u32,
}

impl Default for DwMonitorConfig {
    fn default() -> Self {
        Self {
            mon_log_path: default_mon_log_path(),
            mon_log_interval: default_mon_log_interval(),
            mon_log_file_size: default_mon_log_file_size(),
            mon_log_space_limit: default_mon_log_space_limit(),
        }
    }
}

/// 主备集群完整配置（dw.toml）。
#[derive(Debug, Clone)]
pub struct DwClusterConfig {
    pub oguid: u32,
    /// dmmal.ini MAL 通信层配置（[mal] 段）。
    pub mal: DwMalConfig,
    /// dmwatcher 守护进程配置（[watcher] 段）。
    pub watcher: WatcherConfig,
    /// dmarch.ini 归档配置（[arch] 段）。
    pub arch: DwArchConfig,
    /// 确认监视器模式：true（默认）= MON_DW_CONFIRM=1，需监视器确认才能自动切换；
    /// false = MON_DW_CONFIRM=0，仅通知模式，不参与仲裁。
    pub mon_confirm: bool,
    /// dmmonitor.ini 监视器日志配置（[monitor] 段）。
    pub monitor: DwMonitorConfig,
    pub nodes: Vec<DwNode>,
}

impl DwClusterConfig {
    pub fn primary(&self) -> &DwNode {
        self.nodes
            .iter()
            .find(|n| n.role == NodeRole::Primary)
            .expect("validate_dw_config 已保证恰好一个 primary 节点")
    }

    pub fn standbys(&self) -> impl Iterator<Item = &DwNode> {
        self.nodes.iter().filter(|n| n.role == NodeRole::Standby)
    }

    /// 返回运行 dmmonitor 的节点：优先取第一个 standby，集群无 standby 时 fallback 到 primary。
    /// 官方建议监视器不与 primary 共置，放到备库或独立机器上以避免 primary 故障时监视器同时失联。
    pub fn monitor_node(&self) -> &DwNode {
        self.standbys().next().unwrap_or_else(|| self.primary())
    }
}

// ── TOML 反序列化代理结构体 ──────────────────────────────────────

#[derive(Deserialize)]
struct DwNodeRaw {
    role: NodeRole,
    host: String,
    instance_name: String,
    #[serde(default = "default_install_path")]
    install_path: String,
    #[serde(default = "default_data_path")]
    data_path: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_mal_port")]
    mal_port: u16,
    #[serde(default = "default_dw_port")]
    dw_port: u16,
    #[serde(default = "default_inst_dw_port")]
    inst_dw_port: u16,
    #[serde(default = "default_page_size")]
    page_size: u8,
    #[serde(default = "default_charset")]
    charset: u8,
    #[serde(default = "default_case_sensitive")]
    case_sensitive: bool,
    #[serde(default = "default_extent_size")]
    extent_size: u8,
    #[serde(default)]
    arch_path: Option<String>,
    #[serde(default)]
    sync_mode: StandbyMode,
    #[serde(default = "default_arch_timer_name")]
    arch_timer_name: String,
    #[serde(default)]
    backup: Option<BackupConfig>,
    ssh: SshCredentials,
}

impl From<DwNodeRaw> for DwNode {
    fn from(r: DwNodeRaw) -> Self {
        Self {
            role: r.role,
            host: r.host,
            instance_name: r.instance_name,
            install_path: r.install_path,
            data_path: r.data_path,
            port: r.port,
            mal_port: r.mal_port,
            dw_port: r.dw_port,
            inst_dw_port: r.inst_dw_port,
            page_size: r.page_size,
            charset: r.charset,
            case_sensitive: r.case_sensitive,
            extent_size: r.extent_size,
            arch_path: r.arch_path,
            sync_mode: r.sync_mode,
            arch_timer_name: r.arch_timer_name,
            backup: r.backup,
            ssh: r.ssh,
        }
    }
}

#[derive(Deserialize)]
struct DwClusterConfigRaw {
    #[serde(default = "default_oguid")]
    oguid: u32,
    #[serde(default)]
    mal: DwMalConfig,
    #[serde(default)]
    watcher: WatcherConfig,
    #[serde(default)]
    arch: DwArchConfig,
    #[serde(default = "default_mon_confirm")]
    mon_confirm: bool,
    #[serde(default)]
    monitor: DwMonitorConfig,
    #[serde(rename = "nodes")]
    nodes: Vec<DwNodeRaw>,
}

impl From<DwClusterConfigRaw> for DwClusterConfig {
    fn from(r: DwClusterConfigRaw) -> Self {
        Self {
            oguid: r.oguid,
            mal: r.mal,
            watcher: r.watcher,
            arch: r.arch,
            mon_confirm: r.mon_confirm,
            monitor: r.monitor,
            nodes: r.nodes.into_iter().map(DwNode::from).collect(),
        }
    }
}

fn default_oguid() -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // 从 Unix 时间戳推算 YYYYMMDD
    let days = now / 86400;
    let mut y = 1970u32;
    let mut remaining = days as u32;
    loop {
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days: [u32; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 1u32;
    for &d in &month_days {
        if remaining < d {
            break;
        }
        remaining -= d;
        m += 1;
    }
    let day = remaining + 1;
    y * 10000 + m * 100 + day
}

fn default_mon_confirm() -> bool { true }
fn default_mon_log_path() -> String { ".".to_string() }
fn default_mon_log_interval() -> u32 { 60 }
fn default_mon_log_file_size() -> u32 { 32 }
fn default_mon_log_space_limit() -> u32 { 4096 }
fn default_mal_check_interval() -> u32 { 60 }
fn default_mal_conn_fail_interval() -> u32 { 60 }
fn default_mal_login_timeout() -> u32 { 60 }
fn default_mal_buf_size() -> u32 { 100 }
fn default_mal_sys_buf_size() -> u32 { 0 }
fn default_mal_compress_level() -> u8 { 0 }
fn default_arch_wait_apply() -> u8 { 1 }
fn default_arch_reserve_time() -> u32 { 0 }
fn default_arch_send_policy() -> u8 { 0 }
fn default_arch_recover_time() -> u32 { 60 }
fn default_arch_file_size() -> u32 { 1024 }
fn default_dw_error_time() -> u32 { 60 }
fn default_inst_error_time() -> u32 { 60 }
fn default_inst_recover_time() -> u32 { 60 }
fn default_inst_auto_restart() -> u8 { 0 }
fn default_inst_restart_cnt() -> u32 { 0 }
fn default_dw_open_force_timeout() -> u32 { 0 }
fn default_dw_failover_force() -> u8 { 1 }
fn default_dw_reconnect() -> u8 { 1 }
fn default_rlog_send_threshold() -> u32 { 0 }
fn default_rlog_apply_threshold() -> u32 { 0 }
fn default_inst_service_ip_check() -> u8 { 0 }

fn default_install_path() -> String {
    "/home/dmdba/dmdbms".to_string()
}
fn default_data_path() -> String {
    "/home/dmdba/dmdbms/data".to_string()
}
fn default_port() -> u16 {
    5236
}
fn default_mal_port() -> u16 {
    5237
}
fn default_dw_port() -> u16 {
    5238
}
fn default_inst_dw_port() -> u16 {
    5239
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

/// 从 dw.toml 加载并验证主备集群配置。
pub fn load_dw_specific(path: &Path) -> Result<DwClusterConfig> {
    if !path.exists() {
        bail!("未找到主备集群配置文件 {}", path.display());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取主备集群配置文件: {}", path.display()))?;
    let raw = toml::from_str::<DwClusterConfigRaw>(&content)
        .with_context(|| format!("主备集群配置文件解析失败: {}", path.display()))?;
    let cfg = DwClusterConfig::from(raw);
    validate_dw_config(&cfg)?;
    Ok(cfg)
}

/// 校验 DwClusterConfig 语义合法性。
pub fn validate_dw_config(cfg: &DwClusterConfig) -> Result<()> {
    if cfg.nodes.is_empty() {
        bail!("配置验证失败: dw.toml 节点列表（nodes）不能为空");
    }
    if cfg.oguid > 2_147_483_647 {
        bail!(
            "配置验证失败: oguid 无效: {}；有效范围为 0-2147483647",
            cfg.oguid
        );
    }

    let primary_count = cfg
        .nodes
        .iter()
        .filter(|n| n.role == NodeRole::Primary)
        .count();
    if primary_count != 1 {
        bail!(
            "配置验证失败: 集群必须恰好有 1 个 primary 节点，当前为 {}",
            primary_count
        );
    }

    let mut seen_instance_names = HashSet::new();
    for node in &cfg.nodes {
        validate_db_params(
            "dminit ",
            node.port,
            node.page_size,
            node.charset,
            node.extent_size,
        )?;
        if node.mal_port == node.port {
            bail!(
                "配置验证失败: 节点 {} 的 mal_port 不能与 port 相同: {}",
                node.host,
                node.port
            );
        }
        if node.ssh.identity_file.is_none() && node.ssh.password.is_none() {
            bail!(
                "配置验证失败: 节点 {} 的 ssh 配置必须提供 identity_file 或 password 之一",
                node.host
            );
        }
        if node.role == NodeRole::Primary {
            match node.backup.as_ref().and_then(|b| b.backup_path.as_deref()) {
                None | Some("") => bail!(
                    "配置验证失败: primary 节点 {} 的 backup_path 未配置；请在 dw.toml [[nodes]] 的 [nodes.backup] 段配置 backup_path",
                    node.host
                ),
                _ => {}
            }
            if let Some(b) = &node.backup {
                if b.retain_days < 15 {
                    bail!(
                        "配置验证失败: 节点 {} 的 backup.retain_days 无效: {}；至少保留 15 天",
                        node.host,
                        b.retain_days
                    );
                }
            }
        }
        if !seen_instance_names.insert(node.instance_name.clone()) {
            bail!(
                "配置验证失败: instance_name 在集群内必须唯一，重复值: {}",
                node.instance_name
            );
        }
        if node.role == NodeRole::Standby
            && node.sync_mode == StandbyMode::Async
            && node.arch_timer_name.trim().is_empty()
        {
            bail!(
                "配置验证失败: 节点 {} 为异步备库（sync_mode = async），arch_timer_name 不能为空",
                node.host
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_fixture(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file
    }

    const VALID_TOML: &str = r#"
oguid = 453331

[[nodes]]
role = "primary"
host = "192.168.1.10"
port = 5236
install_path = "/opt/dmdbms"
data_path = "/opt/dmdbms/data"
instance_name = "DM01"

[nodes.backup]
backup_path = "/opt/dmdbms/backup"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[nodes]]
role = "standby"
host = "192.168.1.11"
port = 5236
install_path = "/opt/dmdbms"
data_path = "/opt/dmdbms/data"
instance_name = "DM02"

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;

    #[test]
    fn test_load_dw_specific_valid() {
        let file = write_fixture(VALID_TOML);
        let cfg = load_dw_specific(file.path()).expect("应解析成功");
        assert_eq!(cfg.oguid, 453331);
        assert_eq!(cfg.nodes.len(), 2);
        assert_eq!(cfg.nodes[0].role, NodeRole::Primary);
        assert_eq!(cfg.nodes[1].role, NodeRole::Standby);
        assert_eq!(cfg.nodes[0].mal_port, 5237);
    }

    #[test]
    fn test_load_dw_specific_missing_file_fails() {
        let err = load_dw_specific(Path::new("/nonexistent/dw.toml")).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("未找到主备集群配置文件"));
    }

    #[test]
    fn test_validate_rejects_no_primary() {
        let toml = VALID_TOML.replace("role = \"primary\"", "role = \"standby\"");
        let file = write_fixture(&toml);
        let err = load_dw_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("恰好有 1 个 primary 节点"),
            "实际: {msg}"
        );
    }

    #[test]
    fn test_validate_rejects_two_primary() {
        let toml = VALID_TOML.replace("role = \"standby\"", "role = \"primary\"");
        let file = write_fixture(&toml);
        let err = load_dw_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("恰好有 1 个 primary 节点"), "实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_empty_nodes() {
        let toml = "oguid = 1\nnodes = []\n";
        let file = write_fixture(toml);
        let err = load_dw_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("节点列表（nodes）不能为空"), "实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_oguid_out_of_range() {
        let toml = VALID_TOML.replace("oguid = 453331", "oguid = 3000000000");
        let file = write_fixture(&toml);
        let err = load_dw_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("oguid 无效"), "实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_mal_port_conflict() {
        let toml = VALID_TOML.replacen("port = 5236", "port = 5236\nmal_port = 5236", 1);
        let file = write_fixture(&toml);
        let err = load_dw_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("mal_port 不能与 port 相同"), "实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_missing_ssh_credentials() {
        let toml = VALID_TOML.replace("identity_file = \"~/.ssh/id_rsa\"", "");
        let file = write_fixture(&toml);
        let err = load_dw_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("必须提供 identity_file 或 password 之一"),
            "实际: {msg}"
        );
    }

    #[test]
    fn test_validate_rejects_duplicate_instance_name() {
        let toml = VALID_TOML.replace("DM02", "DM01");
        let file = write_fixture(&toml);
        let err = load_dw_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("instance_name 在集群内必须唯一"),
            "实际: {msg}"
        );
    }

    #[test]
    fn test_validate_rejects_primary_missing_backup_path() {
        let toml = VALID_TOML.replace("[nodes.backup]\nbackup_path = \"/opt/dmdbms/backup\"\n\n", "");
        let file = write_fixture(&toml);
        let err = load_dw_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("backup_path 未配置"), "实际: {msg}");
    }

    #[test]
    fn test_standby_without_backup_config_is_valid() {
        // standby 节点不填 [nodes.backup] 应通过校验
        let file = write_fixture(VALID_TOML);
        let cfg = load_dw_specific(file.path()).expect("standby 无备份配置应合法");
        let standby = cfg.standbys().next().expect("应有 standby");
        assert!(standby.backup.is_none(), "standby.backup 应为 None");
    }

    #[test]
    fn test_validate_rejects_invalid_page_size() {
        let toml = VALID_TOML.replacen(
            "instance_name = \"DM01\"",
            "instance_name = \"DM01\"\npage_size = 12",
            1,
        );
        let file = write_fixture(&toml);
        let err = load_dw_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("page_size 无效: 12"), "实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_async_standby_with_empty_timer_name() {
        let toml = VALID_TOML.replacen(
            "instance_name = \"DM02\"",
            "instance_name = \"DM02\"\nsync_mode = \"async\"\narch_timer_name = \"\"",
            1,
        );
        let file = write_fixture(&toml);
        let err = load_dw_specific(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("arch_timer_name 不能为空"), "实际: {msg}");
    }

    #[test]
    fn test_validate_accepts_async_standby_with_timer_name() {
        let toml = VALID_TOML.replacen(
            "instance_name = \"DM02\"",
            "instance_name = \"DM02\"\nsync_mode = \"async\"",
            1,
        );
        let file = write_fixture(&toml);
        let cfg = load_dw_specific(file.path()).expect("应通过校验");
        let standby = cfg.standbys().next().expect("应有 standby");
        assert_eq!(standby.sync_mode, StandbyMode::Async);
        assert_eq!(standby.arch_timer_name, "RT_TIMER");
    }
}
