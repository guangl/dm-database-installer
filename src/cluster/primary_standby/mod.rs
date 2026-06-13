use anyhow::Result;

use crate::cluster::{deploy, health, preflight};
use crate::common::ssh;
use crate::config::cluster::{ClusterSpecificConfig, NodeConfig};
use crate::config::CommonConfig;

pub async fn run(common: CommonConfig, specific: ClusterSpecificConfig) -> Result<()> {
    use std::sync::Arc;

    tracing::info!("[cluster][1/6] 建立 SSH 会话");
    let mut runners: Vec<(NodeConfig, Arc<dyn ssh::CommandRunner>)> = Vec::new();
    for node in &specific.nodes {
        let session = ssh::SshSession::connect(&node.host, 22, &node.ssh)
            .await
            .map_err(|e| anyhow::anyhow!("连接节点 {} 失败: {}", node.host, e))?;
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
    runners: Vec<(NodeConfig, std::sync::Arc<dyn ssh::CommandRunner>)>,
    health_check_fn: impl Fn(String, u16, u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
        + Send
        + Sync,
) -> Result<()> {
    run_preflight(&runners).await?;
    run_install_phase(&common, &specific, &runners).await?;
    run_distribute_phase(&specific, &runners).await?;
    run_startup_phase(&specific, &runners, &health_check_fn).await?;
    run_watcher_phase(&runners).await?;
    tracing::info!("集群部署完成");
    Ok(())
}

async fn run_preflight(
    runners: &[(NodeConfig, std::sync::Arc<dyn ssh::CommandRunner>)],
) -> Result<()> {
    tracing::info!("[cluster][2/6] SSH 预检查");
    let items: Vec<_> = runners
        .iter()
        .map(|(n, r)| (n.clone(), std::sync::Arc::clone(r)))
        .collect();
    preflight::preflight_all_nodes(items)
        .await
        .map_err(|e| anyhow::anyhow!("预检查失败: {}", e))
}

async fn run_install_phase(
    common: &CommonConfig,
    specific: &ClusterSpecificConfig,
    runners: &[(NodeConfig, std::sync::Arc<dyn ssh::CommandRunner>)],
) -> Result<()> {
    tracing::info!("[cluster][3/6] 并发推送安装包并 dminit");
    let pkg = common.installer_package.as_ref()
        .ok_or_else(|| anyhow::anyhow!("集群安装需要在 config.toml 中指定 installer_package"))?;
    let _ = specific;
    let futs: Vec<_> = runners
        .iter()
        .map(|(node, runner)| {
            let node = node.clone();
            let runner = std::sync::Arc::clone(runner);
            let pkg = pkg.clone();
            async move {
                deploy::upload_installer_and_install(&node, &pkg, runner.as_ref()).await?;
                deploy::run_dminit_remote(&node, runner.as_ref()).await
            }
        })
        .collect();
    futures::future::try_join_all(futs).await?;
    Ok(())
}

async fn run_distribute_phase(
    specific: &ClusterSpecificConfig,
    runners: &[(NodeConfig, std::sync::Arc<dyn ssh::CommandRunner>)],
) -> Result<()> {
    tracing::info!("[cluster][4/6] 分发配置文件");
    let all_nodes: Vec<_> = runners.iter().map(|(n, _)| n.clone()).collect();
    let oguid = specific.oguid;
    let futs: Vec<_> = runners
        .iter()
        .map(|(node, runner)| {
            let node = node.clone();
            let runner = std::sync::Arc::clone(runner);
            let all_nodes = all_nodes.clone();
            async move {
                deploy::distribute_configs(&node, &all_nodes, oguid, runner.as_ref()).await
            }
        })
        .collect();
    futures::future::try_join_all(futs).await?;
    Ok(())
}

async fn run_startup_phase(
    specific: &ClusterSpecificConfig,
    runners: &[(NodeConfig, std::sync::Arc<dyn ssh::CommandRunner>)],
    health_check_fn: &(impl Fn(String, u16, u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> + Send + Sync),
) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][5/6] 有序启动主备实例");
    let (primary_node, primary_runner) = runners
        .iter()
        .find(|(n, _)| n.role == NodeRole::Primary)
        .ok_or_else(|| anyhow::anyhow!("缺少 primary 节点"))?;
    deploy::start_dmserver_mount(primary_node, primary_runner.as_ref()).await?;
    tracing::info!("[node:{:?}] 等待主节点健康 (TCP:{}) ...", primary_node.role, primary_node.port);
    health_check_fn(primary_node.host.clone(), primary_node.port, 60).await?;
    tracing::info!("[node:{:?}] 主节点就绪", primary_node.role);
    deploy::configure_database_role(primary_node, NodeRole::Primary, specific.oguid, primary_runner.as_ref()).await?;
    let (standby_node, standby_runner) = runners
        .iter()
        .find(|(n, _)| n.role == NodeRole::Standby)
        .ok_or_else(|| anyhow::anyhow!("缺少 standby 节点"))?;
    tracing::info!("[node:{:?}][5/6] 启动达梦备实例", standby_node.role);
    deploy::start_dmserver_mount(standby_node, standby_runner.as_ref()).await?;
    health_check_fn(standby_node.host.clone(), standby_node.port, 60).await?;
    deploy::configure_database_role(standby_node, NodeRole::Standby, specific.oguid, standby_runner.as_ref()).await
}

async fn run_watcher_phase(
    runners: &[(NodeConfig, std::sync::Arc<dyn ssh::CommandRunner>)],
) -> Result<()> {
    tracing::info!("[cluster][6/6] 启动 dmwatcher");
    let futs: Vec<_> = runners
        .iter()
        .map(|(node, runner)| {
            let node = node.clone();
            let runner = std::sync::Arc::clone(runner);
            async move { deploy::start_dmwatcher(&node, runner.as_ref()).await }
        })
        .collect();
    futures::future::try_join_all(futs).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ssh::MockRunner;
    use crate::config::cluster::{ClusterSpecificConfig, NodeConfig, NodeRole, SshCredentials};
    use crate::config::{CommonConfig, InstallType};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_node(role: NodeRole, instance_name: &str, host: &str) -> NodeConfig {
        NodeConfig {
            role,
            host: host.to_string(),
            port: 5236,
            instance_name: instance_name.to_string(),
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
                identity_file: None,
                password: Some("pass".to_string()),
            },
        }
    }

    fn make_specific(nodes: Vec<NodeConfig>) -> ClusterSpecificConfig {
        ClusterSpecificConfig { oguid: 453331, nodes, shared_storage: None }
    }

    fn make_common(installer_package: Option<PathBuf>) -> CommonConfig {
        CommonConfig {
            install_type: InstallType::PrimaryStandby,
            installer_package,
            log_level: "info".to_string(),
        }
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
        let runner1 = make_full_runner();
        let runner2 = make_full_runner();
        let runners: Vec<(NodeConfig, Arc<dyn ssh::CommandRunner>)> = vec![
            (standby1, runner1 as Arc<dyn ssh::CommandRunner>),
            (standby2, runner2 as Arc<dyn ssh::CommandRunner>),
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
        let primary_runner = make_full_runner();
        let standby_runner = make_full_runner();
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
