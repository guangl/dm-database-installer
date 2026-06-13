use crate::config::cluster::NodeConfig;

/// 生成 dm.ini 集群追加片段（在现有单机 dm.ini 参数基础上追加）。
///
/// 追加字段说明：
/// - MAL_INI = 1：启用 MAL 系统（多活链路）
/// - ARCH_INI = 1：启用归档
/// - ALTER_MODE_STATUS = 0：初始值 0，SQL 设置主备角色时临时改为 1
/// - ENABLE_OFFLINE_TS = 2：集群模式推荐值
pub fn generate_dm_ini_cluster_suffix(_node: &NodeConfig) -> String {
    "MAL_INI = 1\nARCH_INI = 1\nALTER_MODE_STATUS = 0\nENABLE_OFFLINE_TS = 2\n".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::cluster::{NodeConfig, NodeRole, SshCredentials};
    use std::path::PathBuf;

    fn make_test_node() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Primary,
            host: "192.168.1.10".to_string(),
            port: 5236,
            instance_name: "DMSVR01".to_string(),
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            page_size: 8,
            charset: 0,
            case_sensitive: true,
            extent_size: 16,
            read_only: false,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: Some(PathBuf::from("~/.ssh/id_rsa")),
                password: None,
            },
        }
    }

    #[test]
    fn test_dm_ini_cluster_suffix_contains_required_fields() {
        let node = make_test_node();
        let suffix = generate_dm_ini_cluster_suffix(&node);
        assert!(suffix.contains("MAL_INI = 1"), "缺少 MAL_INI = 1");
        assert!(suffix.contains("ARCH_INI = 1"), "缺少 ARCH_INI = 1");
        assert!(suffix.contains("ALTER_MODE_STATUS = 0"), "缺少 ALTER_MODE_STATUS = 0");
        assert!(suffix.contains("ENABLE_OFFLINE_TS = 2"), "缺少 ENABLE_OFFLINE_TS = 2");
    }
}
