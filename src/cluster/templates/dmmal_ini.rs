use crate::config::cluster::{DminitConfig, MalConfig, NodeConfig};

/// 生成 dmmal.ini 内容（主备节点完全相同，同一 bytes 分发到两个节点）。
///
/// 关键：主备 dmmal.ini 必须字节完全一致，MAL 链路才能建立。
/// 防范 Pitfall 1：调用同一函数同一参数，结果 bytes 必然相等。
pub fn generate_dmmal_ini(nodes: &[NodeConfig], dminit: &DminitConfig, mal: &MalConfig) -> String {
    let mut out = format!(
        "MAL_CHECK_INTERVAL = {}\nMAL_CONN_FAIL_INTERVAL = {}\n\
         MAL_BUF_SIZE = {}\nMAL_SYS_BUF_SIZE = {}\nMAL_COMPRESS_LEVEL = {}\n\n",
        mal.check_interval, mal.conn_fail_interval,
        mal.buf_size, mal.sys_buf_size, mal.compress_level,
    );
    for (i, node) in nodes.iter().enumerate() {
        out.push_str(&format_mal_inst(i, node, dminit));
    }
    out
}

fn format_mal_inst(idx: usize, node: &NodeConfig, dminit: &DminitConfig) -> String {
    format!(
        "[MAL_INST{}]\nMAL_INST_NAME = {}\nMAL_HOST = {}\nMAL_PORT = {}\n\
         MAL_INST_HOST = {}\nMAL_INST_PORT = {}\nMAL_DW_PORT = {}\nMAL_INST_DW_PORT = {}\n\n",
        idx + 1,
        node.instance_name,
        node.host,
        node.mal_port,
        node.host,
        dminit.port,
        node.dw_port,
        node.inst_dw_port,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::cluster::{DminitConfig, MalConfig, NodeConfig, NodeRole, SshCredentials};
    use std::path::PathBuf;

    fn make_dminit() -> DminitConfig {
        DminitConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            port: 5236,
            page_size: 8,
            charset: 0,
            case_sensitive: true,
            extent_size: 16,
            sysdba_password: "SYSDBA".to_string(),
        }
    }

    fn make_primary() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Primary,
            host: "192.168.1.10".to_string(),
            instance_name: "DMSVR01".to_string(),
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

    fn make_standby() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Standby,
            host: "192.168.1.11".to_string(),
            instance_name: "DMSVR02".to_string(),
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

    #[test]
    fn test_dmmal_ini_same_for_both_nodes() {
        let nodes = vec![make_primary(), make_standby()];
        let dminit = make_dminit();
        let mal = MalConfig::default();
        let a = generate_dmmal_ini(&nodes, &dminit, &mal);
        let b = generate_dmmal_ini(&nodes, &dminit, &mal);
        assert_eq!(a, b, "dmmal.ini 主备两次调用结果必须字节相等（Pitfall 1）");
        assert!(a.contains("[MAL_INST1]"), "应含 [MAL_INST1]");
        assert!(a.contains("[MAL_INST2]"), "应含 [MAL_INST2]");
        assert!(a.contains("MAL_INST_NAME = DMSVR01"), "应含主节点实例名");
        assert!(a.contains("MAL_INST_NAME = DMSVR02"), "应含备节点实例名");
        assert!(a.contains("MAL_PORT = 5237"), "应含 MAL_PORT = 5237");
        assert!(a.contains("MAL_DW_PORT = 5238"), "应含 MAL_DW_PORT");
        assert!(a.contains("MAL_INST_DW_PORT = 5239"), "应含 MAL_INST_DW_PORT");
        assert!(a.contains("MAL_INST_PORT = 5236"), "应含 MAL_INST_PORT = 5236");
    }
}
