//! 测试夹具，供 dpc 模块下各文件的单元测试共用。

use crate::config::dpc::{DpcBpGroup, DpcClusterConfig, DpcNode, DpcRole};
use crate::config::ssh::SshCredentials;

pub(super) fn make_node(role: DpcRole, host: &str, instance_name: &str) -> DpcNode {
    DpcNode {
        role,
        host: host.to_string(),
        instance_name: instance_name.to_string(),
        install_path: "/opt/dmdbms".to_string(),
        data_path: "/opt/dmdbms/data".to_string(),
        port: 5236,
        ap_port: 5237,
        raft_group: None,
        raft_self_id: None,
        page_size: 32,
        charset: 1,
        case_sensitive: true,
        extent_size: 32,
        ssh: SshCredentials {
            user: "root".to_string(),
            identity_file: Some("~/.ssh/id_rsa".into()),
            password: None,
        },
    }
}

fn raft_node(host: &str, instance_name: &str, group: &str, self_id: u32) -> DpcNode {
    DpcNode {
        raft_group: Some(group.to_string()),
        raft_self_id: Some(self_id),
        ..make_node(DpcRole::Bp, host, instance_name)
    }
}

/// 单副本集群：1 MP / 2 BP / 1 SP，无 raft_group。mp_port 取 5238 避免与 MP 节点 port=5236 冲突。
pub(super) fn make_single_replica_cluster() -> DpcClusterConfig {
    DpcClusterConfig {
        cluster_id: 20240601,
        mp_host: "192.168.1.10".to_string(),
        mp_port: 5238,
        bp_groups: vec![],
        nodes: vec![
            make_node(DpcRole::Mp, "192.168.1.10", "MP01"),
            make_node(DpcRole::Bp, "192.168.1.11", "BP01"),
            make_node(DpcRole::Bp, "192.168.1.12", "BP02"),
            make_node(DpcRole::Sp, "192.168.1.13", "SP01"),
        ],
    }
}

/// 多副本集群：1 MP / 1 SP / 一个 BP raft_group（2 副本，self_id 1 与 2）。
pub(super) fn make_multi_replica_cluster() -> DpcClusterConfig {
    DpcClusterConfig {
        cluster_id: 20240602,
        mp_host: "192.168.1.10".to_string(),
        mp_port: 5238,
        bp_groups: vec![DpcBpGroup {
            name: "BG1".to_string(),
            rafts: vec!["RAFT1".to_string()],
        }],
        nodes: vec![
            make_node(DpcRole::Mp, "192.168.1.10", "MP01"),
            raft_node("192.168.1.11", "BP01", "RAFT1", 1),
            raft_node("192.168.1.12", "BP02", "RAFT1", 2),
            make_node(DpcRole::Sp, "192.168.1.13", "SP01"),
        ],
    }
}
