use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub use crate::config::ssh::SshCredentials;

/// 集群类型。
#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ClusterType {
    /// 主备集群（默认）
    #[default]
    PrimaryStandby,
    /// 读写分离（基于主备，备节点承担只读查询）
    Rws,
    /// 共享存储集群（多实例共享 SAN/NFS）
    Dsc,
}

/// 节点角色：主节点或备节点。
#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    Primary,
    Standby,
}

/// 单节点配置（主备 / 读写分离 / DSC 共用）。
#[derive(Debug, Deserialize, Clone)]
pub struct NodeConfig {
    pub role: NodeRole,
    pub host: String,
    #[serde(default = "default_port_node")]
    pub port: u16,
    pub instance_name: String,
    #[serde(default = "default_install_path_node")]
    pub install_path: String,
    #[serde(default = "default_data_path_node")]
    pub data_path: String,
    #[serde(default = "default_mal_port_node")]
    pub mal_port: u16,
    #[serde(default = "default_dw_port_node")]
    pub dw_port: u16,
    #[serde(default = "default_inst_dw_port_node")]
    pub inst_dw_port: u16,
    #[serde(default = "default_page_size_node")]
    pub page_size: u8,
    #[serde(default = "default_charset_node")]
    pub charset: u8,
    #[serde(default = "default_case_sensitive_node")]
    pub case_sensitive: bool,
    #[serde(default = "default_extent_size_node")]
    pub extent_size: u8,
    /// 读写分离模式下备节点标记为只读
    #[serde(default)]
    pub read_only: bool,
    pub ssh: SshCredentials,
}

fn default_port_node() -> u16 { 5236 }
fn default_install_path_node() -> String { "/opt/dmdbms".to_string() }
fn default_data_path_node() -> String { "/opt/dmdbms/data".to_string() }
fn default_mal_port_node() -> u16 { 5237 }
fn default_dw_port_node() -> u16 { 5238 }
fn default_inst_dw_port_node() -> u16 { 5239 }
fn default_page_size_node() -> u8 { 8 }
fn default_charset_node() -> u8 { 0 }
fn default_case_sensitive_node() -> bool { true }
fn default_extent_size_node() -> u8 { 16 }


/// 集群顶层配置（对应 TOML `[cluster]` 节）。
#[derive(Debug, Deserialize)]
pub struct ClusterSection {
    /// 集群类型，默认 primary-standby
    #[serde(rename = "type", default)]
    pub cluster_type: ClusterType,
    /// 控制机本地安装包路径
    pub installer_package: PathBuf,
    /// 守护系统全局唯一标识，主备/RWS/DSC 必须相同，默认 453331
    #[serde(default = "default_oguid")]
    pub oguid: u32,
    /// 主备 / 读写分离 / DSC 节点列表（`[[cluster.nodes]]`）
    #[serde(default)]
    pub nodes: Vec<NodeConfig>,
    /// DSC 专用：共享存储路径（SAN 裸设备或 NFS 挂载点）
    pub shared_storage: Option<String>,
}

fn default_oguid() -> u32 { 453331 }

/// 集群配置根结构，对应整个集群 TOML 文件。
#[derive(Debug, Deserialize)]
pub struct ClusterConfig {
    pub cluster: ClusterSection,
}

/// 从 TOML 文件加载集群配置并执行语义验证。
pub fn load_cluster_config(path: &Path) -> Result<ClusterConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取集群配置文件: {}", path.display()))?;
    let cfg = toml::from_str::<ClusterConfig>(&content)
        .with_context(|| "集群配置文件解析失败")?;
    validate_cluster_config(&cfg)?;
    Ok(cfg)
}

/// 验证集群配置的语义合法性，按集群类型分派。
pub fn validate_cluster_config(cfg: &ClusterConfig) -> Result<()> {
    match cfg.cluster.cluster_type {
        ClusterType::PrimaryStandby => validate_primary_standby(cfg),
        ClusterType::Rws => validate_rws(cfg),
        ClusterType::Dsc => validate_dsc(cfg),
    }
}

fn validate_primary_standby(cfg: &ClusterConfig) -> Result<()> {
    check_nodes_not_empty(cfg)?;
    check_role_uniqueness(cfg)?;
    check_oguid_range(cfg)?;
    check_node_fields(cfg)?;
    check_instance_name_uniqueness(cfg)
}

fn validate_rws(cfg: &ClusterConfig) -> Result<()> {
    check_nodes_not_empty(cfg)?;
    check_role_uniqueness(cfg)?;
    check_oguid_range(cfg)?;
    check_node_fields(cfg)?;
    check_instance_name_uniqueness(cfg)?;
    let has_readonly_standby = cfg.cluster.nodes.iter()
        .any(|n| n.role == NodeRole::Standby && n.read_only);
    if !has_readonly_standby {
        bail!("配置验证失败: 读写分离模式要求至少一个备节点设置 read_only = true");
    }
    Ok(())
}

fn validate_dsc(cfg: &ClusterConfig) -> Result<()> {
    check_nodes_not_empty(cfg)?;
    check_role_uniqueness(cfg)?;
    check_oguid_range(cfg)?;
    check_node_fields(cfg)?;
    check_instance_name_uniqueness(cfg)?;
    if cfg.cluster.shared_storage.is_none() {
        bail!("配置验证失败: DSC 集群必须设置 shared_storage（共享存储路径）");
    }
    Ok(())
}


fn check_nodes_not_empty(cfg: &ClusterConfig) -> Result<()> {
    if cfg.cluster.nodes.is_empty() {
        bail!("配置验证失败: 集群必须至少含一个节点");
    }
    Ok(())
}

fn check_role_uniqueness(cfg: &ClusterConfig) -> Result<()> {
    let primary_count = cfg.cluster.nodes.iter()
        .filter(|n| n.role == NodeRole::Primary)
        .count();
    if primary_count != 1 {
        bail!("配置验证失败: 必须恰好一个 primary 节点，当前有 {} 个", primary_count);
    }
    Ok(())
}

fn check_oguid_range(cfg: &ClusterConfig) -> Result<()> {
    if cfg.cluster.oguid > 2_147_483_647 {
        bail!(
            "配置验证失败: oguid 越界: {}；有效范围 0-2147483647",
            cfg.cluster.oguid
        );
    }
    Ok(())
}

fn check_node_fields(cfg: &ClusterConfig) -> Result<()> {
    for node in &cfg.cluster.nodes {
        validate_single_node(node)?;
    }
    Ok(())
}

fn validate_single_node(node: &NodeConfig) -> Result<()> {
    if node.port == 0 {
        bail!("配置验证失败: node[{}] port 无效: 0", node.host);
    }
    if node.mal_port == node.port {
        bail!(
            "配置验证失败: node[{}] mal_port 不能等于 port: {}",
            node.host, node.port
        );
    }
    if node.ssh.identity_file.is_none() && node.ssh.password.is_none() {
        bail!(
            "配置验证失败: node[{}] 至少提供 identity_file 或 password 之一",
            node.host
        );
    }
    if ![4u8, 8, 16, 32].contains(&node.page_size) {
        bail!(
            "配置验证失败: node[{}] page_size 无效: {}；有效值为 4/8/16/32",
            node.host, node.page_size
        );
    }
    if ![0u8, 1, 2].contains(&node.charset) {
        bail!(
            "配置验证失败: node[{}] charset 无效: {}；有效值 0=GB18030 1=UTF-8 2=EUC-KR",
            node.host, node.charset
        );
    }
    if ![16u8, 32].contains(&node.extent_size) {
        bail!(
            "配置验证失败: node[{}] extent_size 无效: {}；有效值为 16/32",
            node.host, node.extent_size
        );
    }
    Ok(())
}

fn check_instance_name_uniqueness(cfg: &ClusterConfig) -> Result<()> {
    let mut seen = HashSet::new();
    for node in &cfg.cluster.nodes {
        if !seen.insert(&node.instance_name) {
            bail!("配置验证失败: instance_name 重复: {}", node.instance_name);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_valid_toml() -> String {
        r#"
[cluster]
installer_package = "/tmp/dm8_setup.iso"
oguid = 453331

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
port = 5236
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
port = 5236
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#.to_string()
    }

    fn write_toml(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", content).unwrap();
        file
    }

    #[test]
    fn test_load_cluster_valid_two_nodes() {
        let file = write_toml(&make_valid_toml());
        let cfg = load_cluster_config(file.path()).expect("应返回 Ok(ClusterConfig)");
        assert_eq!(cfg.cluster.nodes.len(), 2, "应有 2 个节点");
        assert_eq!(cfg.cluster.nodes[0].role, NodeRole::Primary);
        assert_eq!(cfg.cluster.nodes[1].role, NodeRole::Standby);
        assert_eq!(cfg.cluster.installer_package, PathBuf::from("/tmp/dm8_setup.iso"));
        assert_eq!(cfg.cluster.oguid, 453331);
    }

    #[test]
    fn test_load_cluster_valid_instance_names_different() {
        let file = write_toml(&make_valid_toml());
        let cfg = load_cluster_config(file.path()).unwrap();
        assert_eq!(cfg.cluster.nodes[0].instance_name, "DMSVR01");
        assert_eq!(cfg.cluster.nodes[1].instance_name, "DMSVR02");
    }

    #[test]
    fn test_load_cluster_valid_ssh_credentials() {
        let file = write_toml(&make_valid_toml());
        let cfg = load_cluster_config(file.path()).unwrap();
        assert_eq!(cfg.cluster.nodes[0].ssh.user, "root");
        assert_eq!(
            cfg.cluster.nodes[0].ssh.identity_file,
            Some(PathBuf::from("~/.ssh/id_rsa"))
        );
    }

    #[test]
    fn test_load_cluster_valid_default_ports() {
        let toml = r#"
[cluster]
installer_package = "/tmp/dm8_setup.iso"

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let cfg = load_cluster_config(file.path()).unwrap();
        assert_eq!(cfg.cluster.nodes[0].mal_port, 5237, "mal_port 默认 5237");
        assert_eq!(cfg.cluster.nodes[0].dw_port, 5238, "dw_port 默认 5238");
        assert_eq!(cfg.cluster.nodes[0].inst_dw_port, 5239, "inst_dw_port 默认 5239");
    }

    #[test]
    fn test_validate_rejects_no_primary() {
        let toml = r#"
[cluster]
installer_package = "/tmp/dm8_setup.iso"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.10"
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_config(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("必须恰好一个 primary 节点"), "应含 primary 错误，实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_port_conflict() {
        let toml = r#"
[cluster]
installer_package = "/tmp/dm8_setup.iso"

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
port = 5236
mal_port = 5236
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
port = 5236
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_config(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("mal_port 不能等于 port"), "应含端口冲突错误，实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_missing_ssh_credentials() {
        let toml = r#"
[cluster]
installer_package = "/tmp/dm8_setup.iso"

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_config(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("至少提供 identity_file 或 password 之一"),
            "应含 SSH 凭据错误，实际: {msg}"
        );
    }

    #[test]
    fn test_validate_rejects_oguid_overflow() {
        let toml = r#"
[cluster]
installer_package = "/tmp/dm8_setup.iso"
oguid = 2147483648

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_config(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("oguid 越界"), "应含 oguid 越界错误，实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_invalid_page_size() {
        let toml = r#"
[cluster]
installer_package = "/tmp/dm8_setup.iso"

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
page_size = 12
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_config(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("page_size 无效"), "应含 page_size 错误，实际: {msg}");
    }

    #[test]
    fn test_validate_rejects_duplicate_instance_name() {
        let toml = r#"
[cluster]
installer_package = "/tmp/dm8_setup.iso"

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_config(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("instance_name 重复"), "应含实例名重复错误，实际: {msg}");
    }

    #[test]
    fn test_cluster_type_defaults_to_primary_standby() {
        let file = write_toml(&make_valid_toml());
        let cfg = load_cluster_config(file.path()).unwrap();
        assert_eq!(cfg.cluster.cluster_type, ClusterType::PrimaryStandby);
    }

    #[test]
    fn test_rws_requires_readonly_standby() {
        let toml = r#"
[cluster]
type = "rws"
installer_package = "/tmp/dm8_setup.iso"
oguid = 453331

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_config(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("read_only = true"), "应提示设置 read_only，实际: {msg}");
    }

    #[test]
    fn test_rws_accepts_readonly_standby() {
        let toml = r#"
[cluster]
type = "rws"
installer_package = "/tmp/dm8_setup.iso"
oguid = 453331

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
read_only = true
host = "192.168.1.11"
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        assert!(load_cluster_config(file.path()).is_ok(), "读写分离配置应合法");
    }

    #[test]
    fn test_dsc_requires_shared_storage() {
        let toml = r#"
[cluster]
type = "dsc"
installer_package = "/tmp/dm8_setup.iso"
oguid = 453331

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        let err = load_cluster_config(file.path()).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("shared_storage"), "应提示设置 shared_storage，实际: {msg}");
    }

    #[test]
    fn test_dsc_accepts_shared_storage() {
        let toml = r#"
[cluster]
type = "dsc"
installer_package = "/tmp/dm8_setup.iso"
oguid = 453331
shared_storage = "/dev/sdc"

[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;
        let file = write_toml(toml);
        assert!(load_cluster_config(file.path()).is_ok(), "DSC 配置应合法");
    }

}
