use crate::config::cluster::{DwMode, NodeConfig, NodeRole, WatcherConfig};

/// 生成 dmmonitor.ini 内容。
///
/// MON_DW_CONFIRM：AUTO 守护模式下需设为 1（确认模式），MANUAL 设为 0。
/// 每个数据节点（primary/standby）各贡献一对 MON_DW_IP / MON_DW_PORT，
/// monitor 节点本身不参与守护链路，不写入配置。
pub fn generate_dmmonitor_ini(
    all_nodes: &[NodeConfig],
    oguid: u32,
    watcher: &WatcherConfig,
) -> String {
    let confirm = if watcher.dw_mode == DwMode::Auto { 1 } else { 0 };
    let mut ini = format!("MON_DW_CONFIRM = {confirm}\n\n[GRP1]\n MON_INST_OGUID = {oguid}\n");
    for node in all_nodes.iter().filter(|n| matches!(n.role, NodeRole::Primary | NodeRole::Standby)) {
        ini.push_str(&format!(" MON_DW_IP = {}\n MON_DW_PORT = {}\n", node.host, node.dw_port));
    }
    ini
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::cluster::{DwMode, NodeConfig, NodeRole, SshCredentials, WatcherConfig};

    fn make_node(role: NodeRole, host: &str, dw_port: u16) -> NodeConfig {
        NodeConfig {
            role,
            host: host.to_string(),
            instance_name: "DMSVR".to_string(),
            mal_port: 5237,
            dw_port,
            inst_dw_port: 5239,
            read_only: false,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: None,
                password: Some("pass".to_string()),
            },
        }
    }

    #[test]
    fn test_dmmonitor_ini_auto_mode_confirm_is_1() {
        let nodes = vec![make_node(NodeRole::Primary, "192.168.1.10", 5238)];
        let ini = generate_dmmonitor_ini(&nodes, 453331, &WatcherConfig::default());
        assert!(ini.contains("MON_DW_CONFIRM = 1"), "AUTO 模式应含 MON_DW_CONFIRM = 1：{ini}");
    }

    #[test]
    fn test_dmmonitor_ini_manual_mode_confirm_is_0() {
        let nodes = vec![make_node(NodeRole::Primary, "192.168.1.10", 5238)];
        let watcher = WatcherConfig { dw_mode: DwMode::Manual, ..WatcherConfig::default() };
        let ini = generate_dmmonitor_ini(&nodes, 453331, &watcher);
        assert!(ini.contains("MON_DW_CONFIRM = 0"), "MANUAL 模式应含 MON_DW_CONFIRM = 0：{ini}");
    }

    #[test]
    fn test_dmmonitor_ini_contains_all_data_nodes() {
        let nodes = vec![
            make_node(NodeRole::Primary, "192.168.1.10", 5238),
            make_node(NodeRole::Standby, "192.168.1.11", 5238),
        ];
        let ini = generate_dmmonitor_ini(&nodes, 453331, &WatcherConfig::default());
        assert!(ini.contains("MON_DW_IP = 192.168.1.10"), "应含主节点 IP");
        assert!(ini.contains("MON_DW_IP = 192.168.1.11"), "应含备节点 IP");
        assert_eq!(ini.matches("MON_DW_PORT = 5238").count(), 2, "应有 2 个 MON_DW_PORT");
    }

    #[test]
    fn test_dmmonitor_ini_excludes_monitor_node() {
        let nodes = vec![
            make_node(NodeRole::Primary, "192.168.1.10", 5238),
            make_node(NodeRole::Standby, "192.168.1.11", 5238),
            make_node(NodeRole::Monitor, "192.168.1.12", 5238),
        ];
        let ini = generate_dmmonitor_ini(&nodes, 453331, &WatcherConfig::default());
        assert!(!ini.contains("192.168.1.12"), "monitor 节点 IP 不应出现在 dmmonitor.ini：{ini}");
        assert_eq!(ini.matches("MON_DW_IP").count(), 2, "只应有 2 个 MON_DW_IP（primary + standby）");
    }

    #[test]
    fn test_dmmonitor_ini_oguid_matches() {
        let nodes = vec![make_node(NodeRole::Primary, "192.168.1.10", 5238)];
        let ini = generate_dmmonitor_ini(&nodes, 453331, &WatcherConfig::default());
        assert!(ini.contains("MON_INST_OGUID = 453331"), "应含正确 oguid：{ini}");
    }
}
