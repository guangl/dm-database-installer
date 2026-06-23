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

/// 守护切换模式：AUTO = 故障时自动切换主备；MANUAL = 需人工介入切换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum DwMode {
    #[default]
    Auto,
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

/// 主备集群完整配置（dw.toml）。
#[derive(Debug, Clone)]
pub struct DwClusterConfig {
    pub oguid: u32,
    /// 守护切换模式：AUTO（默认，故障自动切换）或 MANUAL（人工介入切换）。
    pub dw_mode: DwMode,
    /// 确认监视器模式：true（默认）= MON_DW_CONFIRM=1，需监视器确认才能自动切换；
    /// false = MON_DW_CONFIRM=0，仅通知模式，不参与仲裁。
    pub mon_confirm: bool,
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
    dw_mode: DwMode,
    #[serde(default = "default_mon_confirm")]
    mon_confirm: bool,
    #[serde(rename = "nodes")]
    nodes: Vec<DwNodeRaw>,
}

impl From<DwClusterConfigRaw> for DwClusterConfig {
    fn from(r: DwClusterConfigRaw) -> Self {
        Self {
            oguid: r.oguid,
            dw_mode: r.dw_mode,
            mon_confirm: r.mon_confirm,
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

fn default_mon_confirm() -> bool {
    true
}

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
}
