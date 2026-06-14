use crate::config::cluster::NodeConfig;

/// 生成 dmarch.ini 内容（主备节点 ARCH_DEST 方向相反）。
///
/// - 主节点：ARCH_DEST = 备节点实例名（实时归档目标为备节点）
/// - 备节点：ARCH_DEST = 主节点实例名（用于角色切换时）
///
/// 本地归档目录基于当前节点的 data_path。
pub fn generate_dmarch_ini(node: &NodeConfig, peer_instance: &str) -> String {
    format!(
        "[ARCHIVE_REALTIME]\nARCH_TYPE = REALTIME\nARCH_DEST = {}\n\n\
         [ARCHIVE_LOCAL1]\nARCH_TYPE = LOCAL\nARCH_DEST = {}/arch\n\
         ARCH_FILE_SIZE = 128\nARCH_SPACE_LIMIT = 0\n",
        peer_instance,
        node.data_path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::cluster::{NodeConfig, NodeRole, SshCredentials};
    use std::path::PathBuf;

    fn make_primary() -> NodeConfig {
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

    fn make_standby() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Standby,
            host: "192.168.1.11".to_string(),
            port: 5236,
            instance_name: "DMSVR02".to_string(),
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
    fn test_dmarch_ini_primary_dest_is_standby() {
        let primary = make_primary();
        let ini = generate_dmarch_ini(&primary, "DMSVR02");
        assert!(ini.contains("ARCH_DEST = DMSVR02"), "主节点 ARCH_DEST 应指向备节点 DMSVR02，实际: {ini}");
        assert!(ini.contains("ARCH_TYPE = REALTIME"), "应含 REALTIME 段");
        assert!(ini.contains("ARCH_TYPE = LOCAL"), "应含 LOCAL 段");
        assert!(ini.contains("ARCH_DEST = /opt/dmdbms/data/arch"), "应含本地归档路径");
    }

    #[test]
    fn test_dmarch_ini_standby_dest_is_primary() {
        let standby = make_standby();
        let ini = generate_dmarch_ini(&standby, "DMSVR01");
        assert!(ini.contains("ARCH_DEST = DMSVR01"), "备节点 ARCH_DEST 应指向主节点 DMSVR01，实际: {ini}");
        // 验证主备 ARCH_DEST 方向相反
        let primary = make_primary();
        let primary_ini = generate_dmarch_ini(&primary, "DMSVR02");
        let primary_dest = "ARCH_DEST = DMSVR02";
        let standby_dest = "ARCH_DEST = DMSVR01";
        assert!(primary_ini.contains(primary_dest), "主节点必须含 {primary_dest}");
        assert!(ini.contains(standby_dest), "备节点必须含 {standby_dest}");
        assert_ne!(primary_dest, standby_dest, "主备 ARCH_DEST 必须不同");
    }
}
