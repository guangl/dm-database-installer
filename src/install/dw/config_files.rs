//! 主备集群守护配置文件生成：dmmal.ini / dmarch.ini / dmwatcher.ini。
//! 字段含义与默认值参考达梦官方主备搭建文档（MAL 通信、REALTIME 归档、DMWatcher 守护）。

use crate::config::dw::{DwClusterConfig, DwNode};

/// dmmal.ini：MAL（镜像通信层）节点列表，所有节点内容相同。
pub fn dmmal_ini(cluster: &DwClusterConfig) -> String {
    let m = &cluster.mal;
    let mut out = format!(
        "MAL_CHECK_INTERVAL    = {mal_check_interval}\n\
         MAL_CONN_FAIL_INTERVAL = {mal_conn_fail_interval}\n\
         MAL_LOGIN_TIMEOUT     = {mal_login_timeout}\n\
         MAL_BUF_SIZE          = {mal_buf_size}\n",
        mal_check_interval = m.mal_check_interval,
        mal_conn_fail_interval = m.mal_conn_fail_interval,
        mal_login_timeout = m.mal_login_timeout,
        mal_buf_size = m.mal_buf_size,
    );
    if m.mal_sys_buf_size > 0 {
        out.push_str(&format!("MAL_SYS_BUF_SIZE      = {}\n", m.mal_sys_buf_size));
    }
    if m.mal_compress_level > 0 {
        out.push_str(&format!("MAL_COMPRESS_LEVEL    = {}\n", m.mal_compress_level));
    }
    out.push('\n');
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
/// `space_limit_mb`：已解析好的本地归档空间上限（MB）。`cluster.arch.arch_space_limit`
/// 为 `None`（自动）时需要查询磁盘容量，属于异步 IO，不在本（纯同步）函数内完成——
/// 由调用方（`config_dist::distribute_config_all`）解析后传入。
pub fn dmarch_ini(node: &DwNode, cluster: &DwClusterConfig, space_limit_mb: u32) -> String {
    let a = &cluster.arch;
    let mut out = format!(
        "ARCH_WAIT_APPLY = {arch_wait_apply}\n\
         ARCH_RESERVE_TIME = {arch_reserve_time}\n\
         ARCH_SEND_POLICY = {arch_send_policy}\n\
         ARCH_RECOVER_TIME = {arch_recover_time}\n\n",
        arch_wait_apply = a.arch_wait_apply,
        arch_reserve_time = a.arch_reserve_time,
        arch_send_policy = a.arch_send_policy,
        arch_recover_time = a.arch_recover_time,
    );
    let peers: Vec<&DwNode> = cluster.nodes.iter().filter(|n| n.host != node.host).collect();
    let same_mode_count = |mode: crate::config::dw::StandbyMode| {
        peers.iter().filter(|p| p.sync_mode == mode).count()
    };
    let mut seen = std::collections::HashMap::new();
    for peer in &peers {
        use crate::config::dw::StandbyMode;
        let arch_type = peer.sync_mode.arch_type();
        let base = match peer.sync_mode {
            StandbyMode::Realtime => "ARCHIVE_REALTIME",
            StandbyMode::Sync => "ARCHIVE_SYNC",
            StandbyMode::Async => "ARCHIVE_ASYNC",
        };
        let section = if same_mode_count(peer.sync_mode) == 1 {
            base.to_string()
        } else {
            let n = seen.entry(peer.sync_mode).or_insert(0);
            *n += 1;
            format!("{base}{n}")
        };
        let extra = match peer.sync_mode {
            StandbyMode::Realtime => "ARCH_FLUSH_BUF_SIZE = 0\n".to_string(),
            StandbyMode::Sync => format!("ARCH_RECOVER_TIME = {}\n", a.arch_recover_time),
            StandbyMode::Async => format!("ARCH_TIMER_NAME = {}\n", peer.arch_timer_name),
        };
        out.push_str(&format!(
            "[{section}]\n\
             ARCH_TYPE = {arch_type}\n\
             ARCH_DEST = {dest}\n\
             {extra}\n",
            dest = peer.instance_name,
        ));
    }
    out.push_str(&format!(
        "[ARCHIVE_LOCAL1]\n\
         ARCH_TYPE = LOCAL\n\
         ARCH_DEST = {arch_path}\n\
         ARCH_FILE_SIZE = {file_size}\n\
         ARCH_SPACE_LIMIT = {space_limit_mb}\n",
        arch_path = node.resolve_arch_path(),
        file_size = a.arch_file_size,
    ));
    out
}

/// dmwatcher.ini：数据守护进程配置，各字段由 cluster.watcher 控制。
pub fn dmwatcher_ini(node: &DwNode, cluster: &DwClusterConfig) -> String {
    let w = &cluster.watcher;
    let dm_ini = crate::install::steps::service::dm_ini_path(&node.as_install_config());
    // Sync/Async 备库的守护是本地守护（DW_TYPE=LOCAL），不参与监视器仲裁/自动切换；
    // 仅 primary 与 Realtime 备库组成的失败切换对使用全局守护（DW_TYPE=GLOBAL）。
    let dw_type = if node.role == crate::config::dw::NodeRole::Primary
        || node.sync_mode == crate::config::dw::StandbyMode::Realtime
    {
        "GLOBAL"
    } else {
        "LOCAL"
    };
    let mut out = format!(
        "[GRP1]\n\
         DW_TYPE          = {dw_type}\n\
         DW_MODE          = {dw_mode}\n\
         DW_ERROR_TIME    = {dw_error_time}\n\
         DW_RECONNECT     = {dw_reconnect}\n\
         DW_FAILOVER_FORCE = {dw_failover_force}\n",
        dw_mode = w.dw_mode.as_str(),
        dw_error_time = w.dw_error_time,
        dw_reconnect = w.dw_reconnect,
        dw_failover_force = w.dw_failover_force,
    );
    if w.dw_open_force_timeout > 0 {
        out.push_str(&format!("DW_OPEN_FORCE_TIMEOUT = {}\n", w.dw_open_force_timeout));
    }
    out.push_str(&format!(
        "INST_OGUID       = {oguid}\n\
         INST_INI         = {dm_ini}\n\
         INST_ERROR_TIME  = {inst_error_time}\n\
         INST_RECOVER_TIME = {inst_recover_time}\n\
         INST_AUTO_RESTART = {inst_auto_restart}\n\
         INST_STARTUP_CMD = {install_path}/bin/dmserver\n",
        oguid = cluster.oguid,
        dm_ini = dm_ini,
        inst_error_time = w.inst_error_time,
        inst_recover_time = w.inst_recover_time,
        inst_auto_restart = w.inst_auto_restart,
        install_path = node.install_path,
    ));
    if w.inst_restart_cnt > 0 {
        out.push_str(&format!("INST_RESTART_CNT = {}\n", w.inst_restart_cnt));
    }
    if w.inst_service_ip_check != 0 {
        out.push_str(&format!("INST_SERVICE_IP_CHECK = {}\n", w.inst_service_ip_check));
    }
    if w.rlog_send_threshold > 0 {
        out.push_str(&format!("RLOG_SEND_THRESHOLD = {}\n", w.rlog_send_threshold));
    }
    if w.rlog_apply_threshold > 0 {
        out.push_str(&format!("RLOG_APPLY_THRESHOLD = {}\n", w.rlog_apply_threshold));
    }
    out.push_str("RLOG_SEND_APPLY_MON = 1\n");
    out
}

/// dmmonitor.ini：监视器确认监视配置，列出 GRP1（DW_TYPE=GLOBAL）组内节点的 MAL_HOST:MAL_DW_PORT。
/// Sync/Async 备库的守护是本地守护（DW_TYPE=LOCAL），不参与监视器仲裁/自动切换，因此不出现在此列表中
/// （需与 `dmwatcher_ini` 中 DW_TYPE 的判定逻辑保持一致）。
/// 简化实现：监视器与某个节点共置运行（由调用方决定，通常是 primary），
/// 不引入独立的监视器主机配置项。
pub fn dmmonitor_ini(cluster: &DwClusterConfig) -> String {
    let mon = &cluster.monitor;
    let mut out = format!(
        "MON_DW_CONFIRM = {}\n\
         MON_LOG_PATH = {mon_log_path}\n\
         MON_LOG_INTERVAL = {mon_log_interval}\n\
         MON_LOG_FILE_SIZE = {mon_log_file_size}\n\
         MON_LOG_SPACE_LIMIT = {mon_log_space_limit}\n\n\
         [GRP1]\n",
        cluster.mon_confirm as u8,
        mon_log_path = mon.mon_log_path,
        mon_log_interval = mon.mon_log_interval,
        mon_log_file_size = mon.mon_log_file_size,
        mon_log_space_limit = mon.mon_log_space_limit,
    );
    out.push_str(&format!("MON_INST_OGUID = {}\n", cluster.oguid));
    for node in cluster.nodes.iter().filter(|n| {
        n.role == crate::config::dw::NodeRole::Primary
            || n.sync_mode == crate::config::dw::StandbyMode::Realtime
    }) {
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
            arch_path: None,
            sync_mode: crate::config::dw::StandbyMode::Realtime,
            arch_timer_name: "RT_TIMER".to_string(),
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
            mal: crate::config::dw::DwMalConfig::default(),
            watcher: crate::config::dw::WatcherConfig::default(),
            arch: crate::config::dw::DwArchConfig::default(),
            mon_confirm: true,
            monitor: crate::config::dw::DwMonitorConfig::default(),
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
        assert!(ini.contains("MAL_CHECK_INTERVAL    = 60"));
        assert!(ini.contains("MAL_BUF_SIZE          = 100"));
        assert!(ini.contains("MAL_INST_NAME = DM01"));
        assert!(ini.contains("MAL_INST_NAME = DM02"));
        assert!(ini.contains("MAL_HOST = 192.168.1.10"));
        assert!(ini.contains("MAL_PORT = 5237"));
    }

    #[test]
    fn test_dmarch_ini_points_to_peer() {
        let cluster = make_cluster();
        let primary_ini = dmarch_ini(&cluster.nodes[0], &cluster, 1024);
        assert!(primary_ini.contains("ARCH_WAIT_APPLY = 1"));
        assert!(primary_ini.contains("ARCH_DEST = DM02"));
        assert!(primary_ini.contains("ARCH_TYPE = REALTIME"));
        assert!(primary_ini.contains("/home/dmdba/dmdbms/data/arch"));
        assert!(primary_ini.contains("ARCH_FILE_SIZE = 1024"));

        let standby_ini = dmarch_ini(&cluster.nodes[1], &cluster, 1024);
        assert!(standby_ini.contains("ARCH_DEST = DM01"));
    }

    #[test]
    fn test_dmwatcher_ini_contains_oguid_and_paths() {
        let cluster = make_cluster();
        let ini = dmwatcher_ini(&cluster.nodes[0], &cluster);
        assert!(ini.contains("INST_OGUID       = 453331"));
        assert!(ini.contains("DW_MODE          = MANUAL")); // 默认手动切换
        assert!(ini.contains("/home/dmdba/dmdbms/bin/dmserver"));
    }

    #[test]
    fn test_dmarch_ini_uses_unsuffixed_section_for_single_peer() {
        let cluster = make_cluster();
        let ini = dmarch_ini(&cluster.nodes[0], &cluster, 1024);
        assert!(ini.contains("[ARCHIVE_REALTIME]"));
        assert!(!ini.contains("[ARCHIVE_REALTIME1]"));
    }

    #[test]
    fn test_dmarch_ini_uses_async_type_for_async_standby() {
        let mut cluster = make_cluster();
        cluster.nodes[1].sync_mode = crate::config::dw::StandbyMode::Async;

        let primary_ini = dmarch_ini(&cluster.nodes[0], &cluster, 1024);
        assert!(primary_ini.contains("[ARCHIVE_ASYNC]"));
        assert!(primary_ini.contains("ARCH_TYPE = ASYNC"));
        assert!(primary_ini.contains("ARCH_DEST = DM02"));
        assert!(primary_ini.contains("ARCH_TIMER_NAME = RT_TIMER"));
        assert!(!primary_ini.contains("ARCH_TYPE = REALTIME"));
    }

    #[test]
    fn test_dmarch_ini_uses_sync_type_for_sync_standby() {
        let mut cluster = make_cluster();
        cluster.nodes[1].sync_mode = crate::config::dw::StandbyMode::Sync;

        let primary_ini = dmarch_ini(&cluster.nodes[0], &cluster, 1024);
        assert!(primary_ini.contains("[ARCHIVE_SYNC]"));
        assert!(primary_ini.contains("ARCH_TYPE = SYNC"));
        assert!(primary_ini.contains("ARCH_DEST = DM02"));
        assert!(primary_ini.contains("ARCH_RECOVER_TIME = 60"));
        assert!(!primary_ini.contains("ARCH_TYPE = REALTIME"));
    }

    #[test]
    fn test_dmmonitor_ini_lists_all_nodes_dw_ports() {
        let cluster = make_cluster();
        let ini = dmmonitor_ini(&cluster);
        assert!(ini.contains("MON_INST_OGUID = 453331"));
        assert!(ini.contains("MON_DW_IP = 192.168.1.10:5238"));
        assert!(ini.contains("MON_DW_IP = 192.168.1.11:5238"));
    }

    #[test]
    fn test_dmmonitor_ini_excludes_sync_and_async_standbys() {
        let mut cluster = make_cluster();
        cluster.nodes[1].sync_mode = crate::config::dw::StandbyMode::Async;

        let ini = dmmonitor_ini(&cluster);
        assert!(ini.contains("MON_DW_IP = 192.168.1.10:5238"));
        assert!(!ini.contains("MON_DW_IP = 192.168.1.11:5238"));
    }

    #[test]
    fn test_dmwatcher_ini_uses_local_type_for_async_standby() {
        let mut cluster = make_cluster();
        cluster.nodes[1].sync_mode = crate::config::dw::StandbyMode::Async;

        let primary_ini = dmwatcher_ini(&cluster.nodes[0], &cluster);
        assert!(primary_ini.contains("DW_TYPE          = GLOBAL"));

        let standby_ini = dmwatcher_ini(&cluster.nodes[1], &cluster);
        assert!(standby_ini.contains("DW_TYPE          = LOCAL"));
    }
}
