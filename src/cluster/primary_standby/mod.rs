use anyhow::Result;

use crate::cluster::{health, phases};
use crate::common::ssh;
use crate::config::cluster::ClusterSpecificConfig;
use crate::config::CommonConfig;

pub async fn run(common: CommonConfig, specific: ClusterSpecificConfig) -> Result<()> {
    use std::sync::Arc;

    tracing::info!("[cluster][1/11] 建立 SSH 会话");
    let mut runners: phases::Runners = Vec::new();
    for node in &specific.nodes {
        let session = ssh::SshSession::connect(&node.host, node.ssh.port, &node.ssh)
            .await
            .map_err(|e| anyhow::anyhow!("连接节点 {}:{} 失败: {}", node.host, node.ssh.port, e))?;
        runners.push((node.clone(), Arc::new(session)));
    }
    run_with_runners(
        common,
        specific,
        runners,
        |host, port, secs| {
            Box::pin(async move { health::wait_tcp_ready(&host, port, secs).await })
        },
    )
    .await
}

pub async fn run_with_runners(
    common: CommonConfig,
    specific: ClusterSpecificConfig,
    runners: phases::Runners,
    health_check_fn: impl Fn(String, u16, u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
        + Send
        + Sync,
) -> Result<()> {
    let dminit = specific.dminit.clone();
    phases::run_preflight(&runners, &dminit).await?;
    phases::run_install_phase(&common, &runners, &dminit).await?;
    phases::run_primary_init_phase(&runners, &health_check_fn, &dminit).await?;
    phases::run_backup_phase(&runners, &dminit).await?;
    phases::run_standby_restore_phase(&runners, &dminit).await?;
    phases::run_distribute_phase(&specific, &runners, &dminit).await?;
    phases::run_startup_phase(&specific, &runners, &health_check_fn, &dminit).await?;
    phases::run_watcher_phase(&runners, &dminit).await?;
    phases::run_monitor_phase(&specific, &runners, &dminit).await?;
    phases::run_sqllog_phase(&specific, &runners, &dminit).await?;
    phases::run_verify_phase(&runners, &dminit).await?;
    tracing::info!("集群部署完成");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ssh::MockRunner;
    use crate::config::cluster::{ClusterSpecificConfig, DminitConfig, NodeConfig, NodeRole, SshCredentials};
    use crate::config::{CommonConfig, InstallType};
    use std::path::PathBuf;
    use std::sync::Arc;

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

    fn make_node(role: NodeRole, instance_name: &str, host: &str) -> NodeConfig {
        NodeConfig {
            role,
            host: host.to_string(),
            instance_name: instance_name.to_string(),
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            read_only: false,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: None,
                password: Some("pass".to_string()),
                port: 22,
            },
        }
    }

    fn make_specific(nodes: Vec<NodeConfig>) -> ClusterSpecificConfig {
        use crate::config::cluster::{ArchiveConfig, DmIniConfig, MalConfig, SqlLogConfig, WatcherConfig};
        ClusterSpecificConfig {
            oguid: 453331,
            nodes,
            dsc_storage: None,
            shared_storage: None,
            dminit: make_dminit(),
            dm_ini: DmIniConfig::default(),
            archive: ArchiveConfig::default(),
            mal: MalConfig::default(),
            watcher: WatcherConfig::default(),
            sqllog: SqlLogConfig::default(),
        }
    }

    fn make_common(installer_package: Option<PathBuf>) -> CommonConfig {
        use crate::config::InstallerSource;
        let installer = match installer_package {
            Some(path) => InstallerSource::LocalFile(path),
            None => InstallerSource::Auto,
        };
        CommonConfig { install_type: InstallType::Dw, installer }
    }

    fn df_output() -> Vec<u8> {
        b"Filesystem  1B-blocks  Used  Available  Use%  Mounted on\n/dev/sda1  100000000000  50000000000  10737418240  50%  /opt\n".to_vec()
    }

    fn make_full_runner() -> Arc<MockRunner> {
        Arc::new(MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("ss -tlnp | grep ':5236'".to_string(), 1, vec![]),
            ("df -B1 /opt".to_string(), 0, df_output()),
        ]))
    }

    #[tokio::test]
    async fn test_run_rejects_no_primary_fixture() {
        let standby1 = make_node(NodeRole::Standby, "DMSVR01", "192.168.1.10");
        let standby2 = make_node(NodeRole::Standby, "DMSVR02", "192.168.1.11");
        let specific = make_specific(vec![standby1.clone(), standby2.clone()]);
        let tmp_pkg = tempfile::NamedTempFile::new().unwrap();
        let common = make_common(Some(tmp_pkg.path().to_path_buf()));
        let runners: Vec<(NodeConfig, Arc<dyn ssh::CommandRunner>)> = vec![
            (standby1, make_full_runner() as Arc<dyn ssh::CommandRunner>),
            (standby2, make_full_runner() as Arc<dyn ssh::CommandRunner>),
        ];
        let result = run_with_runners(common, specific, runners, |_h, _p, _s| Box::pin(async { Ok(()) })).await;
        assert!(result.is_err(), "缺少 primary 节点应返回 Err");
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("primary"), "错误消息应含 'primary': {msg}");
    }

    #[tokio::test]
    async fn test_run_aborts_on_preflight_failure_before_install() {
        let tmp_pkg = tempfile::NamedTempFile::new().unwrap();
        let primary = make_node(NodeRole::Primary, "DMSVR01", "192.168.1.10");
        let standby = make_node(NodeRole::Standby, "DMSVR02", "192.168.1.11");
        let specific = make_specific(vec![primary.clone(), standby.clone()]);
        let common = make_common(Some(tmp_pkg.path().to_path_buf()));
        let primary_runner = Arc::new(MockRunner::new(vec![
            ("sudo -n true".to_string(), 1, vec![]),
        ]));
        let standby_runner = make_full_runner();
        let runners: Vec<(NodeConfig, Arc<dyn ssh::CommandRunner>)> = vec![
            (primary.clone(), primary_runner.clone() as Arc<dyn ssh::CommandRunner>),
            (standby.clone(), standby_runner as Arc<dyn ssh::CommandRunner>),
        ];
        let result = run_with_runners(common, specific, runners, |_h, _p, _s| Box::pin(async { Ok(()) })).await;
        assert!(result.is_err(), "预检查失败应返回 Err");
        let log = primary_runner.exec_log();
        assert!(!log.iter().any(|c| c.contains("dminit")), "预检查失败后不应执行 dminit: {:?}", log);
        assert!(!log.iter().any(|c| c.contains("dmserver")), "预检查失败后不应启动 dmserver: {:?}", log);
        assert!(!log.iter().any(|c| c.contains("disql")), "预检查失败后不应执行 disql: {:?}", log);
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_run_orders_primary_health_before_standby_start() {
        let tmp_pkg = tempfile::NamedTempFile::new().unwrap();
        let primary = make_node(NodeRole::Primary, "DMSVR01", "192.168.1.10");
        let standby = make_node(NodeRole::Standby, "DMSVR02", "192.168.1.11");
        let specific = make_specific(vec![primary.clone(), standby.clone()]);
        let common = make_common(Some(tmp_pkg.path().to_path_buf()));
        let primary_runner = Arc::new(MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("ss -tlnp | grep ':5236'".to_string(), 1, vec![]),
            ("df -B1 /opt".to_string(), 0, df_output()),
            (
                "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
                0,
                b"STATUS$   MODE$\nOPEN      PRIMARY\n".to_vec(),
            ),
        ]));
        let standby_runner = Arc::new(MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("ss -tlnp | grep ':5236'".to_string(), 1, vec![]),
            ("df -B1 /opt".to_string(), 0, df_output()),
            (
                "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
                0,
                b"STATUS$   MODE$\nMOUNT     STANDBY\n".to_vec(),
            ),
        ]));
        let runners: Vec<(NodeConfig, Arc<dyn ssh::CommandRunner>)> = vec![
            (primary.clone(), primary_runner.clone() as Arc<dyn ssh::CommandRunner>),
            (standby.clone(), standby_runner.clone() as Arc<dyn ssh::CommandRunner>),
        ];
        let result = run_with_runners(common, specific, runners, |_h, _p, _s| Box::pin(async { Ok(()) })).await;
        assert!(result.is_ok(), "全部通过应返回 Ok: {:?}", result.err());
        let p_log = primary_runner.exec_log();
        assert!(p_log.iter().any(|c| c.contains("alter database primary")), "primary 应执行 alter database primary: {:?}", p_log);
        let s_log = standby_runner.exec_log();
        assert!(s_log.iter().any(|c| c.contains("mount")), "standby 应执行 mount 启动: {:?}", s_log);
        logs_assert(|lines: &[&str]| {
            let primary_ready = lines.iter().position(|l| l.contains("主节点就绪"));
            let standby_start = lines.iter().position(|l| l.contains("启动达梦备实例"));
            match (primary_ready, standby_start) {
                (Some(a), Some(b)) if a < b => Ok(()),
                _ => Err(format!(
                    "主节点就绪 must precede 启动达梦备实例; ready={:?}, start={:?}",
                    primary_ready, standby_start
                )),
            }
        });
    }
}
