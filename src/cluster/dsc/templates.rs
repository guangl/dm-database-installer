use crate::config::cluster::{DminitConfig, DscStorageConfig, NodeConfig};

/// 生成 dmdcr_cfg.ini 内容（所有节点相同）。
///
/// 包含 CSS、ASM、DB 三个 [GRP] 段，各段端口按节点索引递增。
pub fn generate_dmdcr_cfg_ini(
    nodes: &[NodeConfig],
    oguid: u32,
    storage: &DscStorageConfig,
    dminit: &DminitConfig,
) -> String {
    todo!("实现 dmdcr_cfg.ini 生成")
}

/// 生成 dmasvrmal.ini 内容（所有节点相同）。
///
/// 每个节点对应一个 [MAL_INSTn] 段，端口从 9349 开始每节点递增 2。
pub fn generate_dmasvrmal_ini(nodes: &[NodeConfig]) -> String {
    todo!("实现 dmasvrmal.ini 生成")
}

/// 生成 dmdcr.ini 内容（各节点不同，DMDCR_SEQNO 按节点索引区分）。
///
/// Pitfall 3：SEQNO 必须唯一，按节点在节点列表中的下标设置。
pub fn generate_dmdcr_ini(
    node_index: usize,
    install_path: &str,
    dsc_conf_dir: &str,
    data_path: &str,
    instance_name: &str,
    storage: &DscStorageConfig,
) -> String {
    todo!("实现 dmdcr.ini 生成")
}

/// 生成 dminit.ini 内容（仅 first_node 使用）。
///
/// Pitfall 4：SYSTEM_PATH 和 LOG_PATH 必须以 + 开头代表 ASM 磁盘组。
pub fn generate_dminit_ini(
    nodes: &[NodeConfig],
    dminit: &DminitConfig,
    oguid: u32,
    storage: &DscStorageConfig,
) -> String {
    todo!("实现 dminit.ini 生成")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::cluster::{NodeConfig, NodeRole, SshCredentials};
    use std::path::PathBuf;

    fn make_node(index: usize, role: NodeRole) -> NodeConfig {
        let host = format!("192.168.1.{}", 10 + index);
        let instance_name = format!("DSC{}", index);
        NodeConfig {
            role,
            host,
            instance_name,
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            read_only: false,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: Some(PathBuf::from("~/.ssh/id_rsa")),
                password: None,
                port: 22,
            },
        }
    }

    fn make_dminit() -> DminitConfig {
        DminitConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            port: 5236,
            page_size: 8,
            charset: 0,
            case_sensitive: true,
            extent_size: 16,
            sysdba_password: "Dm123456".to_string(),
        }
    }

    fn make_dsc_storage() -> DscStorageConfig {
        DscStorageConfig {
            dcr_disk: "/dev/raw/raw1".to_string(),
            vote_disk: "/dev/raw/raw2".to_string(),
            log_disk: "/dev/raw/raw3".to_string(),
            data_disk: "/dev/raw/raw4".to_string(),
        }
    }

    fn make_two_nodes() -> Vec<NodeConfig> {
        vec![
            make_node(0, NodeRole::Primary),
            make_node(1, NodeRole::Standby),
        ]
    }

    // Task 1 Tests: generate_dmdcr_cfg_ini

    #[test]
    fn test_dmdcr_cfg_ini_contains_three_grps() {
        let nodes = make_two_nodes();
        let storage = make_dsc_storage();
        let dminit = make_dminit();
        let result = generate_dmdcr_cfg_ini(&nodes, 63635, &storage, &dminit);
        assert!(result.contains("DCR_GRP_TYPE = CSS"), "应含 CSS 段");
        assert!(result.contains("DCR_GRP_TYPE = ASM"), "应含 ASM 段");
        assert!(result.contains("DCR_GRP_TYPE = DB"), "应含 DB 段");
    }

    #[test]
    fn test_dmdcr_cfg_ini_n_grp_and_oguid() {
        let nodes = make_two_nodes();
        let storage = make_dsc_storage();
        let dminit = make_dminit();
        let result = generate_dmdcr_cfg_ini(&nodes, 63635, &storage, &dminit);
        assert!(result.contains("DCR_N_GRP = 3"), "顶部应含 DCR_N_GRP = 3");
        assert!(result.contains("DCR_OGUID = 63635"), "应含 DCR_OGUID = 63635");
        assert!(result.contains("DCR_VTD_PATH = /dev/raw/raw2"), "应含表决磁盘路径");
    }

    #[test]
    fn test_dmdcr_cfg_ini_each_grp_has_n_ep() {
        let nodes = make_two_nodes();
        let storage = make_dsc_storage();
        let dminit = make_dminit();
        let result = generate_dmdcr_cfg_ini(&nodes, 63635, &storage, &dminit);
        let n_ep_count = result.matches("DCR_GRP_N_EP = 2").count();
        assert_eq!(n_ep_count, 3, "三个 [GRP] 段均应含 DCR_GRP_N_EP = 2，共出现 3 次，实际: {}", n_ep_count);
    }

    #[test]
    fn test_dmdcr_cfg_ini_css_ports() {
        let nodes = make_two_nodes();
        let storage = make_dsc_storage();
        let dminit = make_dminit();
        let result = generate_dmdcr_cfg_ini(&nodes, 63635, &storage, &dminit);
        assert!(result.contains("DCR_EP_PORT = 9341"), "CSS 段节点 0 端口应为 9341");
        assert!(result.contains("DCR_EP_PORT = 9343"), "CSS 段节点 1 端口应为 9343");
    }

    #[test]
    fn test_dmdcr_cfg_ini_asm_ports_and_shmkey() {
        let nodes = make_two_nodes();
        let storage = make_dsc_storage();
        let dminit = make_dminit();
        let result = generate_dmdcr_cfg_ini(&nodes, 63635, &storage, &dminit);
        assert!(result.contains("DCR_EP_ASM_LOAD_PATH = /dev/raw"), "ASM 段应含 LOAD_PATH");
        assert!(result.contains("DCR_EP_PORT = 9349"), "ASM 段节点 0 端口应为 9349");
        assert!(result.contains("DCR_EP_PORT = 9351"), "ASM 段节点 1 端口应为 9351");
        assert!(result.contains("DCR_EP_ASM_SHMKEY = 93360"), "ASM 段节点 0 SHMKEY 应为 93360");
        assert!(result.contains("DCR_EP_ASM_SHMKEY = 93361"), "ASM 段节点 1 SHMKEY 应为 93361");
    }

    #[test]
    fn test_dmdcr_cfg_ini_db_ports() {
        let nodes = make_two_nodes();
        let storage = make_dsc_storage();
        let dminit = make_dminit();
        let result = generate_dmdcr_cfg_ini(&nodes, 63635, &storage, &dminit);
        // dminit.port = 5236; DB 段节点 0 = 5236 + 0 = 5236，节点 1 = 5236 + 1 = 5237
        assert!(result.contains("DCR_EP_PORT = 5236"), "DB 段节点 0 端口应为 5236");
        assert!(result.contains("DCR_EP_PORT = 5237"), "DB 段节点 1 端口应为 5237");
    }

    // Task 1 Tests: generate_dmasvrmal_ini

    #[test]
    fn test_dmasvrmal_ini_contains_inst_blocks() {
        let nodes = make_two_nodes();
        let result = generate_dmasvrmal_ini(&nodes);
        assert!(result.contains("[MAL_INST0]"), "应含 [MAL_INST0]");
        assert!(result.contains("[MAL_INST1]"), "应含 [MAL_INST1]");
    }

    #[test]
    fn test_dmasvrmal_ini_inst_name_matches_node() {
        let nodes = make_two_nodes();
        let result = generate_dmasvrmal_ini(&nodes);
        assert!(result.contains("MAL_INST_NAME = DSC0"), "应含 DSC0 实例名");
        assert!(result.contains("MAL_INST_NAME = DSC1"), "应含 DSC1 实例名");
    }

    #[test]
    fn test_dmasvrmal_ini_port_matches_asm_port() {
        let nodes = make_two_nodes();
        let result = generate_dmasvrmal_ini(&nodes);
        assert!(result.contains("MAL_PORT = 9349"), "节点 0 MAL_PORT 应为 9349");
        assert!(result.contains("MAL_PORT = 9351"), "节点 1 MAL_PORT 应为 9351");
    }

    // Task 2 Tests: generate_dmdcr_ini

    #[test]
    fn test_dmdcr_ini_seqno_differs_per_node() {
        let storage = make_dsc_storage();
        let ini0 = generate_dmdcr_ini(0, "/opt/dmdbms", "/opt/dmdbms/dsc_conf", "/opt/dmdbms/data", "DSC0", &storage);
        let ini1 = generate_dmdcr_ini(1, "/opt/dmdbms", "/opt/dmdbms/dsc_conf", "/opt/dmdbms/data", "DSC1", &storage);
        assert!(ini0.contains("DMDCR_SEQNO = 0"), "节点 0 SEQNO 应为 0");
        assert!(ini1.contains("DMDCR_SEQNO = 1"), "节点 1 SEQNO 应为 1");
    }

    #[test]
    fn test_dmdcr_ini_paths_and_intervals() {
        let storage = make_dsc_storage();
        let result = generate_dmdcr_ini(0, "/opt/dmdbms", "/opt/dmdbms/dsc_conf", "/opt/dmdbms/data", "DSC0", &storage);
        assert!(result.contains("DMDCR_PATH = /dev/raw/raw1"), "应含 DCR 磁盘路径");
        assert!(result.contains("DMDCR_MAL_PATH = /opt/dmdbms/dsc_conf/dmasvrmal.ini"), "应含 MAL 路径");
        assert!(result.contains("DMDCR_ASM_RESTART_INTERVAL = 60"), "应含 ASM 重启间隔");
        assert!(result.contains("DMDCR_DB_RESTART_INTERVAL = 60"), "应含 DB 重启间隔");
    }

    #[test]
    fn test_dmdcr_ini_startup_cmds_use_install_path() {
        let storage = make_dsc_storage();
        let result = generate_dmdcr_ini(0, "/opt/dmdbms", "/opt/dmdbms/dsc_conf", "/opt/dmdbms/data", "DSC0", &storage);
        assert!(
            result.contains("DMDCR_ASM_STARTUP_CMD = /opt/dmdbms/bin/dmasmsvr DCR_INI=/opt/dmdbms/dsc_conf/dmdcr.ini"),
            "ASM 启动命令应含正确路径，实际:\n{}", result
        );
        assert!(
            result.contains("DMDCR_DB_STARTUP_CMD = /opt/dmdbms/bin/dmserver /opt/dmdbms/data/DSC0/dm.ini dcr_ini=/opt/dmdbms/dsc_conf/dmdcr.ini"),
            "DB 启动命令应含正确路径，实际:\n{}", result
        );
    }

    // Task 2 Tests: generate_dminit_ini

    #[test]
    fn test_dminit_ini_asm_path_prefix() {
        let nodes = make_two_nodes();
        let dminit = make_dminit();
        let storage = make_dsc_storage();
        let result = generate_dminit_ini(&nodes, &dminit, 63635, &storage);
        assert!(result.contains("SYSTEM_PATH = +DMDATA/data"), "SYSTEM_PATH 应有 + 前缀（Pitfall 4）");
        assert!(result.contains("LOG_PATH = +DMLOG/log/dsc0_log01.log"), "LOG_PATH 应有 + 前缀");
    }

    #[test]
    fn test_dminit_ini_per_node_blocks() {
        let nodes = make_two_nodes();
        let dminit = make_dminit();
        let storage = make_dsc_storage();
        let result = generate_dminit_ini(&nodes, &dminit, 63635, &storage);
        assert!(result.contains("[DSC0]"), "应含 [DSC0] 段");
        assert!(result.contains("[DSC1]"), "应含 [DSC1] 段");
        // dminit.port = 5236; DSC0 PORT = 5236 + 0 = 5236，DSC1 PORT = 5236 + 1 = 5237
        assert!(result.contains("PORT_NUM = 5236"), "DSC0 端口应为 5236");
        assert!(result.contains("PORT_NUM = 5237"), "DSC1 端口应为 5237");
    }

    #[test]
    fn test_dminit_ini_config_path_per_node() {
        let nodes = make_two_nodes();
        let dminit = make_dminit();
        let storage = make_dsc_storage();
        let result = generate_dminit_ini(&nodes, &dminit, 63635, &storage);
        assert!(result.contains("dsc0_config"), "[DSC0] CONFIG_PATH 应含 dsc0_config");
        assert!(result.contains("dsc1_config"), "[DSC1] CONFIG_PATH 应含 dsc1_config");
    }

    #[test]
    fn test_dminit_ini_sysdba_pwd_from_config() {
        let nodes = make_two_nodes();
        let dminit = make_dminit();
        let storage = make_dsc_storage();
        let result = generate_dminit_ini(&nodes, &dminit, 63635, &storage);
        assert!(result.contains("SYSDBA_PWD = Dm123456"), "应含 SYSDBA 密码");
    }
}
