//! 主备集群守护配置文件生成：dmmal.ini / dmarch.ini / dmwatcher.ini。
//! 字段含义与默认值参考达梦官方主备搭建文档（MAL 通信、REALTIME 归档、DMWatcher 守护）。

use crate::config::dw::{DwClusterConfig, DwNode};

/// dmmal.ini：MAL（镜像通信层）节点列表，所有节点内容相同。
pub fn dmmal_ini(cluster: &DwClusterConfig) -> String {
    let mut out = String::from("MAL_CHECK_INTERVAL = 5\nMAL_CONN_FAIL_INTERVAL = 5\n\n");
    for (idx, node) in cluster.nodes.iter().enumerate() {
        out.push_str(&format!(
            "[MAL_INST{n}]\n\
             MAL_INST_NAME = {name}\n\
             MAL_HOST = {host}\n\
             MAL_PORT = {mal_port}\n\
             MAL_DW_PORT = {dw_port}\n\
             MAL_INST_HOST = {host}\n\
             MAL_INST_PORT = {port}\n\
             MAL_INST_DW_PORT = {inst_dw_port}\n\n",
            n = idx + 1,
            name = node.instance_name,
            host = node.host,
            mal_port = node.mal_port,
            dw_port = node.dw_port,
            port = node.port,
            inst_dw_port = node.inst_dw_port,
        ));
    }
    out
}

/// dmarch.ini：本节点的本地归档 + 指向集群内其他节点的实时归档（REALTIME）。
/// 两节点场景下官方示例 section 名不带编号（`[ARCHIVE_REALTIME]`）；
/// 超过一个对端（多备）时才用编号区分（`[ARCHIVE_REALTIME1]`、`[ARCHIVE_REALTIME2]`...）。
pub fn dmarch_ini(node: &DwNode, cluster: &DwClusterConfig) -> String {
    let peers: Vec<&DwNode> = cluster.nodes.iter().filter(|n| n.host != node.host).collect();
    let mut out = String::new();
    for (idx, peer) in peers.iter().enumerate() {
        let section = if peers.len() == 1 {
            "ARCHIVE_REALTIME".to_string()
        } else {
            format!("ARCHIVE_REALTIME{}", idx + 1)
        };
        out.push_str(&format!(
            "[{section}]\n\
             ARCH_TYPE = REALTIME\n\
             ARCH_DEST = {dest}\n\
             ARCH_FLUSH_BUF_SIZE = 0\n\n",
            dest = peer.instance_name,
        ));
    }
    out.push_str(&format!(
        "[ARCHIVE_LOCAL1]\n\
         ARCH_TYPE = LOCAL\n\
         ARCH_DEST = {arch_path}\n\
         ARCH_FILE_SIZE = 1024\n\
         ARCH_SPACE_LIMIT = 0\n",
        arch_path = format!("{}/arch", node.data_path),
    ));
    out
}

/// dmwatcher.ini：数据守护进程配置，AUTO 模式下故障时自动切换。
pub fn dmwatcher_ini(node: &DwNode, cluster: &DwClusterConfig) -> String {
    format!(
        "[GRP1]\n\
         DW_TYPE = GLOBAL\n\
         DW_MODE = AUTO\n\
         DW_ERROR_TIME = 10\n\
         INST_RECOVER_TIME = 60\n\
         INST_OGUID = {oguid}\n\
         INST_INI = {dm_ini}\n\
         INST_AUTO_RESTART = 1\n\
         INST_STARTUP_CMD = {install_path}/bin/dmserver\n\
         RLOG_SEND_APPLY_MON = 1\n",
        oguid = cluster.oguid,
        dm_ini = crate::install::steps::service::dm_ini_path(&node.as_install_config()),
        install_path = node.install_path,
    )
}

/// dmmonitor.ini：监视器确认监视配置，列出集群所有节点的 MAL_HOST:MAL_DW_PORT。
/// 简化实现：监视器与某个节点共置运行（由调用方决定，通常是 primary），
/// 不引入独立的监视器主机配置项。
pub fn dmmonitor_ini(cluster: &DwClusterConfig) -> String {
    let mut out = String::from(
        "MON_DW_CONFIRM = 1\n\
         MON_LOG_PATH = .\n\
         MON_LOG_INTERVAL = 60\n\
         MON_LOG_FILE_SIZE = 32\n\
         MON_LOG_SPACE_LIMIT = 0\n\n\
         [GRP1]\n",
    );
    out.push_str(&format!("MON_INST_OGUID = {}\n", cluster.oguid));
    for node in &cluster.nodes {
        out.push_str(&format!("MON_DW_IP = {}:{}\n", node.host, node.dw_port));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dw::NodeRole;
    use crate::config::ssh::SshCredentials;

    fn make_node(role: NodeRole, host: &str, instance_name: &str) -> DwNode {
        DwNode {
            role,
            host: host.to_string(),
            instance_name: instance_name.to_string(),
            install_path: "/home/dmdba/dmdbms".to_string(),
            data_path: "/home/dmdba/dmdbms/data".to_string(),
            port: 5236,
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            page_size: 32,
            charset: 1,
            case_sensitive: true,
            extent_size: 32,
            backup: if role == NodeRole::Primary {
                Some(crate::config::BackupConfig {
                    backup_path: Some("/home/dmdba/dmdbms/backup".to_string()),
                    ..Default::default()
                })
            } else {
                None
            },
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: Some("~/.ssh/id_rsa".into()),
                password: None,
            },
        }
    }

    fn make_cluster() -> DwClusterConfig {
        DwClusterConfig {
            oguid: 453331,
            nodes: vec![
                make_node(NodeRole::Primary, "192.168.1.10", "DM01"),
                make_node(NodeRole::Standby, "192.168.1.11", "DM02"),
            ],
        }
    }

    #[test]
    fn test_dmmal_ini_lists_all_nodes() {
        let cluster = make_cluster();
        let ini = dmmal_ini(&cluster);
        assert!(ini.contains("MAL_INST_NAME = DM01"));
        assert!(ini.contains("MAL_INST_NAME = DM02"));
        assert!(ini.contains("MAL_HOST = 192.168.1.10"));
        assert!(ini.contains("MAL_PORT = 5237"));
    }

    #[test]
    fn test_dmarch_ini_points_to_peer() {
        let cluster = make_cluster();
        let primary_ini = dmarch_ini(&cluster.nodes[0], &cluster);
        assert!(primary_ini.contains("ARCH_DEST = DM02"));
        assert!(primary_ini.contains("ARCH_TYPE = REALTIME"));
        assert!(primary_ini.contains("/home/dmdba/dmdbms/data/arch"));

        let standby_ini = dmarch_ini(&cluster.nodes[1], &cluster);
        assert!(standby_ini.contains("ARCH_DEST = DM01"));
    }

    #[test]
    fn test_dmwatcher_ini_contains_oguid_and_paths() {
        let cluster = make_cluster();
        let ini = dmwatcher_ini(&cluster.nodes[0], &cluster);
        assert!(ini.contains("INST_OGUID = 453331"));
        assert!(ini.contains("DW_MODE = AUTO"));
        assert!(ini.contains("/home/dmdba/dmdbms/bin/dmserver"));
    }

    #[test]
    fn test_dmarch_ini_uses_unsuffixed_section_for_single_peer() {
        let cluster = make_cluster();
        let ini = dmarch_ini(&cluster.nodes[0], &cluster);
        assert!(ini.contains("[ARCHIVE_REALTIME]"));
        assert!(!ini.contains("[ARCHIVE_REALTIME1]"));
    }

    #[test]
    fn test_dmmonitor_ini_lists_all_nodes_dw_ports() {
        let cluster = make_cluster();
        let ini = dmmonitor_ini(&cluster);
        assert!(ini.contains("MON_INST_OGUID = 453331"));
        assert!(ini.contains("MON_DW_IP = 192.168.1.10:5238"));
        assert!(ini.contains("MON_DW_IP = 192.168.1.11:5238"));
    }
}
