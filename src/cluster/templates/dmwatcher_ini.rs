use crate::config::cluster::NodeConfig;

/// 生成 dmwatcher.ini 内容。
///
/// 关键约束：
/// - INST_OGUID 主备必须相同（Pitfall 5）
/// - INST_INI 路径各节点指向自身的 dm.ini（Pitfall 3）
///   格式：{data_path}/{instance_name}/dm.ini
pub fn generate_dmwatcher_ini(node: &NodeConfig, oguid: u32) -> String {
    format!(
        "[GRP1]\nDW_TYPE = GLOBAL\nDW_MODE = AUTO\nDW_ERROR_TIME = 10\n\
         INST_RECOVER_TIME = 60\nINST_ERROR_TIME = 10\nINST_OGUID = {}\n\
         INST_INI = {}/{}/dm.ini\nINST_AUTO_RESTART = 1\n\
         INST_STARTUP_CMD = {}/bin/dmserver\nRLOG_SEND_THRESHOLD = 0\nRLOG_APPLY_THRESHOLD = 0\n",
        oguid,
        node.data_path,
        node.instance_name,
        node.install_path,
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
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: Some(PathBuf::from("~/.ssh/id_rsa")),
                password: None,
            },
        }
    }

    #[test]
    fn test_dmwatcher_ini_primary_inst_ini_path() {
        let primary = make_primary();
        let ini = generate_dmwatcher_ini(&primary, 453331);
        assert!(
            ini.contains("INST_OGUID = 453331"),
            "应含 INST_OGUID = 453331，实际: {ini}"
        );
        assert!(
            ini.contains("INST_INI = /opt/dmdbms/data/DMSVR01/dm.ini"),
            "主节点 INST_INI 应含 DMSVR01/dm.ini，实际: {ini}"
        );
    }

    #[test]
    fn test_dmwatcher_ini_standby_inst_ini_path_is_own() {
        let standby = make_standby();
        let ini = generate_dmwatcher_ini(&standby, 453331);
        // 防范 Pitfall 3：备节点 INST_INI 必须指向自身（DMSVR02），不能指向主节点（DMSVR01）
        assert!(
            ini.contains("DMSVR02/dm.ini"),
            "备节点 INST_INI 应含 DMSVR02/dm.ini（Pitfall 3），实际: {ini}"
        );
        assert!(
            !ini.contains("DMSVR01/dm.ini"),
            "备节点 INST_INI 不能含 DMSVR01/dm.ini"
        );
    }

    #[test]
    fn test_dmwatcher_ini_oguid_consistent() {
        let primary = make_primary();
        let standby = make_standby();
        let primary_ini = generate_dmwatcher_ini(&primary, 453331);
        let standby_ini = generate_dmwatcher_ini(&standby, 453331);
        // 防范 Pitfall 5：主备 INST_OGUID 必须相同
        assert!(primary_ini.contains("INST_OGUID = 453331"), "主节点 OGUID 应为 453331");
        assert!(standby_ini.contains("INST_OGUID = 453331"), "备节点 OGUID 应为 453331");
        // OGUID 值在两个文件中严格一致
        let primary_oguid = extract_oguid(&primary_ini);
        let standby_oguid = extract_oguid(&standby_ini);
        assert_eq!(primary_oguid, standby_oguid, "主备 INST_OGUID 必须严格相等（Pitfall 5）");
    }

    fn extract_oguid(ini: &str) -> &str {
        ini.lines()
            .find(|l| l.starts_with("INST_OGUID"))
            .map(|l| l.split('=').nth(1).map(|s| s.trim()).unwrap_or(""))
            .unwrap_or("")
    }
}
