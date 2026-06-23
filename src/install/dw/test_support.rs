//! 测试夹具，供 dw 模块下各文件的单元测试共用。

use crate::config::BackupConfig;
use crate::config::dw::{DwClusterConfig, DwNode, NodeRole};
use crate::config::ssh::SshCredentials;

pub(super) fn make_node(role: NodeRole, host: &str, instance_name: &str) -> DwNode {
    DwNode {
        role,
        host: host.to_string(),
        instance_name: instance_name.to_string(),
        install_path: "/opt/dmdbms".to_string(),
        data_path: "/opt/dmdbms/data".to_string(),
        port: 5236,
        mal_port: 5237,
        dw_port: 5238,
        inst_dw_port: 5239,
        page_size: 8,
        charset: 0,
        case_sensitive: true,
        extent_size: 16,
        backup: if role == NodeRole::Primary {
            Some(BackupConfig {
                backup_path: Some("/opt/dmdbms/backup".to_string()),
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

pub(super) fn make_cluster() -> DwClusterConfig {
    DwClusterConfig {
        oguid: 453331,
        nodes: vec![
            make_node(NodeRole::Primary, "192.168.1.10", "DM01"),
            make_node(NodeRole::Standby, "192.168.1.11", "DM02"),
        ],
    }
}
