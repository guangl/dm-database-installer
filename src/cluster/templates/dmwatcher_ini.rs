use crate::config::cluster::{DwMode, DminitConfig, NodeConfig, WatcherConfig};

/// 生成 dmwatcher.ini 内容。
///
/// 关键约束：
/// - INST_OGUID 主备必须相同（Pitfall 5）
/// - INST_INI 路径各节点指向自身的 dm.ini（Pitfall 3）
///   格式：{data_path}/{instance_name}/dm.ini
pub fn generate_dmwatcher_ini(
    node: &NodeConfig,
    dminit: &DminitConfig,
    oguid: u32,
    watcher: &WatcherConfig,
) -> String {
    let dw_mode_str = match watcher.dw_mode {
        DwMode::Auto => "AUTO",
        DwMode::Manual => "MANUAL",
    };
    let default_startup_cmd = format!("{}/bin/dmserver", dminit.install_path);
    let startup_cmd = watcher.inst_startup_cmd.as_deref().unwrap_or(&default_startup_cmd);
    format!(
        "[GRP1]\nDW_TYPE = GLOBAL\nDW_MODE = {}\nDW_ERROR_TIME = {}\n\
         INST_RECOVER_TIME = {}\nINST_ERROR_TIME = {}\nINST_OGUID = {}\n\
         INST_INI = {}/{}/dm.ini\nINST_AUTO_RESTART = {}\n\
         INST_STARTUP_CMD = {}\nRLOG_SEND_THRESHOLD = {}\nRLOG_APPLY_THRESHOLD = {}\n",
        dw_mode_str,
        watcher.dw_error_time,
        watcher.inst_recover_time,
        watcher.inst_error_time,
        oguid,
        dminit.data_path,
        node.instance_name,
        watcher.inst_auto_restart,
        startup_cmd,
        watcher.rlog_send_threshold,
        watcher.rlog_apply_threshold,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::cluster::{DminitConfig, NodeConfig, NodeRole, SshCredentials, WatcherConfig};
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
    fn test_dmwatcher_ini_primary_inst_ini_path() {
        let primary = make_primary();
        let dminit = make_dminit();
        let watcher = WatcherConfig::default();
        let ini = generate_dmwatcher_ini(&primary, &dminit, 453331, &watcher);
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
        let dminit = make_dminit();
        let watcher = WatcherConfig::default();
        let ini = generate_dmwatcher_ini(&standby, &dminit, 453331, &watcher);
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
        let dminit = make_dminit();
        let watcher = WatcherConfig::default();
        let primary_ini = generate_dmwatcher_ini(&primary, &dminit, 453331, &watcher);
        let standby_ini = generate_dmwatcher_ini(&standby, &dminit, 453331, &watcher);
        // 防范 Pitfall 5：主备 INST_OGUID 必须相同
        assert!(primary_ini.contains("INST_OGUID = 453331"), "主节点 OGUID 应为 453331");
        assert!(standby_ini.contains("INST_OGUID = 453331"), "备节点 OGUID 应为 453331");
        let primary_oguid = extract_oguid(&primary_ini);
        let standby_oguid = extract_oguid(&standby_ini);
        assert_eq!(primary_oguid, standby_oguid, "主备 INST_OGUID 必须严格相等（Pitfall 5）");
    }

    #[test]
    fn test_dmwatcher_ini_manual_mode() {
        use crate::config::cluster::DwMode;
        let node = make_primary();
        let dminit = make_dminit();
        let watcher = WatcherConfig {
            dw_mode: DwMode::Manual,
            dw_error_time: 15,
            inst_recover_time: 120,
            inst_error_time: 20,
            inst_auto_restart: 1,
            rlog_send_threshold: 0,
            rlog_apply_threshold: 0,
            inst_startup_cmd: None,
        };
        let ini = generate_dmwatcher_ini(&node, &dminit, 453331, &watcher);
        assert!(ini.contains("DW_MODE = MANUAL"), "应含 DW_MODE = MANUAL");
        assert!(ini.contains("DW_ERROR_TIME = 15"), "应含自定义 dw_error_time");
        assert!(ini.contains("INST_RECOVER_TIME = 120"), "应含自定义 inst_recover_time");
    }

    #[test]
    fn test_dmwatcher_ini_custom_startup_cmd() {
        let node = make_primary();
        let dminit = make_dminit();
        let watcher = WatcherConfig {
            inst_startup_cmd: Some("/custom/path/dmserver".to_string()),
            ..WatcherConfig::default()
        };
        let ini = generate_dmwatcher_ini(&node, &dminit, 453331, &watcher);
        assert!(ini.contains("INST_STARTUP_CMD = /custom/path/dmserver"), "应使用自定义启动命令");
    }

    fn extract_oguid(ini: &str) -> &str {
        ini.lines()
            .find(|l| l.starts_with("INST_OGUID"))
            .map(|l| l.split('=').nth(1).map(|s| s.trim()).unwrap_or(""))
            .unwrap_or("")
    }
}
