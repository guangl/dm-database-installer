use anyhow::Result;
use std::sync::Arc;

use crate::cluster::{deploy, preflight, templates};
use crate::common::ssh;
use crate::config::cluster::{ClusterSpecificConfig, DminitConfig, NodeConfig};
use crate::config::CommonConfig;

pub type Runners = Vec<(NodeConfig, Arc<dyn ssh::CommandRunner>)>;

pub async fn run_preflight(runners: &Runners, dminit: &DminitConfig) -> Result<()> {
    tracing::info!("[cluster][2/11] SSH 预检查");
    let items: Vec<_> = runners
        .iter()
        .map(|(n, r)| (n.clone(), Arc::clone(r)))
        .collect();
    preflight::preflight_all_nodes(items, dminit)
        .await
        .map_err(|e| anyhow::anyhow!("预检查失败: {}", e))
}

pub async fn run_install_phase(
    common: &CommonConfig,
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    use crate::config::cluster::NodeRole;
    use crate::config::InstallerSource;
    tracing::info!("[cluster][3/11] 安装软件包（所有节点并行）+ 主节点 dminit");

    let handle = match &common.installer {
        InstallerSource::Auto => {
            tracing::info!("自动检测本地平台并下载安装包（集群节点默认与控制机平台一致）");
            crate::common::download::fetch_dm_installer().await?
        }
        InstallerSource::LocalFile(path) => crate::common::download::PackageHandle::from_path(path.clone()),
        InstallerSource::Url(url) => {
            tracing::info!("下载安装包 (installer_url): {}", url);
            crate::common::download::fetch_from_url(url).await?
        }
    };
    let pkg_path = handle.path.clone();

    let install_futs: Vec<_> = runners.iter().map(|(node, runner)| {
        let node = node.clone();
        let runner = Arc::clone(runner);
        let pkg = pkg_path.clone();
        let dminit = dminit.clone();
        async move { deploy::upload_installer_and_install(&node, &dminit, &pkg, runner.as_ref()).await }
    }).collect();
    futures::future::try_join_all(install_futs).await?;

    let (primary_node, primary_runner) = runners.iter()
        .find(|(n, _)| n.role == NodeRole::Primary)
        .ok_or_else(|| anyhow::anyhow!("缺少 primary 节点"))?;
    deploy::run_dminit_remote(primary_node, dminit, primary_runner.as_ref()).await
}

pub async fn run_primary_init_phase(
    runners: &Runners,
    health_check_fn: &(impl Fn(String, u16, u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> + Send + Sync),
    dminit: &DminitConfig,
) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][4/11] 主节点首次正常启动（初始化内部结构）");
    let (primary_node, primary_runner) = runners.iter()
        .find(|(n, _)| n.role == NodeRole::Primary)
        .ok_or_else(|| anyhow::anyhow!("缺少 primary 节点"))?;
    deploy::start_dmserver_normal(primary_node, dminit, primary_runner.as_ref()).await?;
    health_check_fn(primary_node.host.clone(), dminit.port, 60).await?;
    tracing::info!("[cluster][4/11] 主节点初始化完成，停止实例");
    deploy::stop_dmserver(primary_node, dminit, primary_runner.as_ref()).await
}

pub async fn run_backup_phase(runners: &Runners, dminit: &DminitConfig) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][5/11] 主节点脱机全量备份");
    let (primary_node, primary_runner) = runners.iter()
        .find(|(n, _)| n.role == NodeRole::Primary)
        .ok_or_else(|| anyhow::anyhow!("缺少 primary 节点"))?;
    let backup_dir = format!("/tmp/dm_backup_{}", primary_node.instance_name);
    deploy::run_dmrman_backup(primary_node, dminit, &backup_dir, primary_runner.as_ref()).await
}

pub async fn run_standby_restore_phase(runners: &Runners, dminit: &DminitConfig) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][6/11] 备节点初始化（dminit + 备份还原）");
    let (primary_node, primary_runner) = runners.iter()
        .find(|(n, _)| n.role == NodeRole::Primary)
        .ok_or_else(|| anyhow::anyhow!("缺少 primary 节点"))?;
    let (standby_node, standby_runner) = runners.iter()
        .find(|(n, _)| n.role == NodeRole::Standby)
        .ok_or_else(|| anyhow::anyhow!("缺少 standby 节点"))?;

    deploy::run_dminit_remote(standby_node, dminit, standby_runner.as_ref()).await?;

    let backup_dir = format!("/tmp/dm_backup_{}", primary_node.instance_name);
    tracing::info!("[cluster][6/11] 从主节点下载备份集");
    let backup_files = deploy::download_backup_files(primary_runner.as_ref(), &backup_dir).await?;
    tracing::info!("[cluster][6/11] 上传备份集到备节点（{} 个文件）", backup_files.len());
    deploy::upload_backup_files(standby_runner.as_ref(), &backup_dir, &backup_files).await?;

    deploy::run_dmrman_restore(standby_node, dminit, &backup_dir, standby_runner.as_ref()).await
}

pub async fn run_distribute_phase(
    specific: &ClusterSpecificConfig,
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][7/11] 分发配置文件");
    let all_nodes: Vec<_> = runners
        .iter()
        .filter(|(n, _)| matches!(n.role, NodeRole::Primary | NodeRole::Standby))
        .map(|(n, _)| n.clone())
        .collect();
    let oguid = specific.oguid;
    let archive = specific.archive.clone();
    let mal = specific.mal.clone();
    let watcher = specific.watcher.clone();
    let futs: Vec<_> = runners
        .iter()
        .filter(|(n, _)| matches!(n.role, NodeRole::Primary | NodeRole::Standby))
        .map(|(node, runner)| {
            let node = node.clone();
            let runner = Arc::clone(runner);
            let all_nodes = all_nodes.clone();
            let archive = archive.clone();
            let mal = mal.clone();
            let watcher = watcher.clone();
            let dminit = dminit.clone();
            async move {
                deploy::distribute_configs(
                    &node, &dminit, &all_nodes, oguid, &archive, &mal, &watcher, runner.as_ref(),
                )
                .await
            }
        })
        .collect();
    futures::future::try_join_all(futs).await?;
    Ok(())
}

pub async fn run_startup_phase(
    specific: &ClusterSpecificConfig,
    runners: &Runners,
    health_check_fn: &(impl Fn(String, u16, u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> + Send + Sync),
    dminit: &DminitConfig,
) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][8/11] 有序启动主备实例");
    let (primary_node, primary_runner) = runners
        .iter()
        .find(|(n, _)| n.role == NodeRole::Primary)
        .ok_or_else(|| anyhow::anyhow!("缺少 primary 节点"))?;
    deploy::start_dmserver_mount(primary_node, dminit, primary_runner.as_ref()).await?;
    tracing::info!("[node:{:?}] 等待主节点健康 (TCP:{}) ...", primary_node.role, dminit.port);
    health_check_fn(primary_node.host.clone(), dminit.port, 60).await?;
    tracing::info!("[node:{:?}] 主节点就绪", primary_node.role);
    deploy::configure_database_role(primary_node, dminit, NodeRole::Primary, specific.oguid, primary_runner.as_ref()).await?;
    let (standby_node, standby_runner) = runners
        .iter()
        .find(|(n, _)| n.role == NodeRole::Standby)
        .ok_or_else(|| anyhow::anyhow!("缺少 standby 节点"))?;
    tracing::info!("[node:{:?}][5/6] 启动达梦备实例", standby_node.role);
    deploy::start_dmserver_mount(standby_node, dminit, standby_runner.as_ref()).await?;
    health_check_fn(standby_node.host.clone(), dminit.port, 60).await?;
    deploy::configure_database_role(standby_node, dminit, NodeRole::Standby, specific.oguid, standby_runner.as_ref()).await
}

pub async fn run_watcher_phase(runners: &Runners, dminit: &DminitConfig) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][9/11] 启动 dmwatcher");
    let futs: Vec<_> = runners
        .iter()
        .filter(|(n, _)| matches!(n.role, NodeRole::Primary | NodeRole::Standby))
        .map(|(node, runner)| {
            let node = node.clone();
            let runner = Arc::clone(runner);
            let dminit = dminit.clone();
            async move { deploy::register_and_start_dmwatcher_service(&node, &dminit, runner.as_ref()).await }
        })
        .collect();
    futures::future::try_join_all(futs).await?;
    Ok(())
}

pub async fn run_monitor_phase(
    specific: &ClusterSpecificConfig,
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][10/11] 启动 dmmonitor");

    let (monitor_node, monitor_runner) = runners
        .iter()
        .find(|(n, _)| n.role == NodeRole::Monitor)
        .or_else(|| runners.iter().find(|(n, _)| n.role == NodeRole::Standby))
        .ok_or_else(|| anyhow::anyhow!("找不到可运行 dmmonitor 的节点（无 monitor 也无 standby）"))?;

    if monitor_node.role == NodeRole::Monitor {
        tracing::info!("[cluster][10/11] 使用专用 monitor 节点 {}", monitor_node.host);
    } else {
        tracing::info!("[cluster][10/11] 未配置 monitor 节点，在备库 {} 上启动 dmmonitor", monitor_node.host);
    }

    let data_nodes: Vec<_> = runners
        .iter()
        .filter(|(n, _)| matches!(n.role, NodeRole::Primary | NodeRole::Standby))
        .map(|(n, _)| n.clone())
        .collect();

    let ini_content = templates::generate_dmmonitor_ini(&data_nodes, specific.oguid, &specific.watcher);
    monitor_runner
        .sftp_write("/tmp/dmmonitor.ini", ini_content.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("上传 dmmonitor.ini 失败: {}", e))?;

    deploy::register_and_start_dmmonitor_service(dminit, "/tmp/dmmonitor.ini", monitor_runner.as_ref()).await
}

pub async fn run_sqllog_phase(
    specific: &ClusterSpecificConfig,
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    if !specific.sqllog.enabled {
        return Ok(());
    }
    tracing::info!("[cluster] 配置 SQL 日志（等待数据库 open）");
    let sqllog = specific.sqllog.clone();
    let futs: Vec<_> = runners
        .iter()
        .filter(|(node, _)| !node.read_only)   // 只读备库跳过 SQL 日志配置
        .map(|(node, runner)| {
            let node = node.clone();
            let runner = Arc::clone(runner);
            let sqllog = sqllog.clone();
            let dminit = dminit.clone();
            async move { deploy::configure_sqllog(&node, &dminit, &sqllog, runner.as_ref()).await }
        })
        .collect();
    futures::future::try_join_all(futs).await?;
    Ok(())
}

pub async fn run_verify_phase(runners: &Runners, dminit: &DminitConfig) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][11/11] 验证主备角色状态");
    let futs: Vec<_> = runners
        .iter()
        .filter(|(n, _)| matches!(n.role, NodeRole::Primary | NodeRole::Standby))
        .map(|(node, runner)| {
            let node = node.clone();
            let runner = Arc::clone(runner);
            let dminit = dminit.clone();
            async move { deploy::verify_node_role(&node, &dminit, node.role, runner.as_ref()).await }
        })
        .collect();
    futures::future::try_join_all(futs).await?;
    Ok(())
}

pub(crate) const MAX_RETRIES: u32 = 24;
pub(crate) const POLL_INTERVAL_SECS: u64 = 5;

async fn wait_for_standby_open_impl(
    node: &NodeConfig,
    dminit: &DminitConfig,
    runner: &dyn ssh::CommandRunner,
    max_retries: u32,
    interval_secs: u64,
) -> Result<()> {
    use crate::common::shell_quote;
    let cmd = format!(
        "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
        shell_quote(&dminit.install_path),
        shell_quote(&dminit.sysdba_password),
        dminit.port,
    );
    for attempt in 1..=max_retries {
        let (stdout, _) = runner
            .exec(&cmd)
            .await
            .map_err(|e| anyhow::anyhow!("poll V$INSTANCE 失败: {}", e))?;
        let output = String::from_utf8_lossy(&stdout);
        if output.contains("OPEN") && output.contains("STANDBY") {
            tracing::info!("[node:{:?}] 只读备库已就绪 STATUS$=OPEN MODE$=STANDBY", node.role);
            return Ok(());
        }
        if attempt < max_retries {
            tracing::warn!(
                "[node:{:?}] 备库尚未 OPEN（{}/{}），{}s 后重试",
                node.role, attempt, max_retries, interval_secs
            );
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
        }
    }
    anyhow::bail!(
        "备节点 {} 未在 {}s 内达到 STATUS$=OPEN MODE$=STANDBY",
        node.host,
        max_retries as u64 * interval_secs
    )
}

async fn wait_for_standby_open(
    node: &NodeConfig,
    dminit: &DminitConfig,
    runner: &dyn ssh::CommandRunner,
) -> Result<()> {
    wait_for_standby_open_impl(node, dminit, runner, MAX_RETRIES, POLL_INTERVAL_SECS).await
}

pub async fn run_read_routing_phase(
    specific: &ClusterSpecificConfig,
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    use crate::config::cluster::NodeRole;
    let _ = specific;
    tracing::info!("[cluster][12/12] 等待只读备库进入 OPEN 状态");
    let readonly_standbys: Vec<_> = runners
        .iter()
        .filter(|(node, _)| node.role == NodeRole::Standby && node.read_only)
        .collect();
    if readonly_standbys.is_empty() {
        tracing::warn!("[cluster][12/12] 无 read_only=true 的备节点，跳过只读验证");
        return Ok(());
    }
    for (node, runner) in &readonly_standbys {
        wait_for_standby_open(node, dminit, runner.as_ref()).await?;
    }
    tracing::info!("[cluster][12/12] 所有只读备库就绪");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::common::ssh::MockRunner;
    use crate::config::cluster::{DminitConfig, NodeConfig, NodeRole, SshCredentials};

    fn make_standby_node(host: &str, read_only: bool) -> NodeConfig {
        NodeConfig {
            role: NodeRole::Standby,
            host: host.to_string(),
            instance_name: "DMSVR02".to_string(),
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            read_only,
            ssh: SshCredentials { user: "root".to_string(), identity_file: None, password: Some("pass".to_string()), port: 22 },
        }
    }

    fn make_dminit() -> DminitConfig {
        DminitConfig {
            install_path: "/opt/dmdbms".to_string(),
            port: 5236,
            ..Default::default()
        }
    }

    fn make_specific() -> ClusterSpecificConfig {
        toml::from_str("").expect("ClusterSpecificConfig 最小空 TOML 解析失败")
    }

    #[tokio::test]
    async fn test_run_read_routing_phase_success() {
        let node = make_standby_node("192.168.1.2", true);
        let dminit = make_dminit();
        let runner = Arc::new(MockRunner::new(vec![(
            "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
            0,
            b"STATUS$   MODE$\nOPEN      STANDBY\n".to_vec(),
        )]));
        let runners: Runners = vec![(node, runner)];
        let specific = make_specific();
        let result = run_read_routing_phase(&specific, &runners, &dminit).await;
        assert!(result.is_ok(), "期望 Ok(()), 实际: {:?}", result);
    }

    #[tokio::test]
    async fn test_run_read_routing_phase_timeout() {
        let node = make_standby_node("192.168.1.2", true);
        let dminit = make_dminit();
        let runner = Arc::new(MockRunner::new(vec![]));
        let result =
            wait_for_standby_open_impl(&node, &dminit, runner.as_ref(), 2, 0).await;
        assert!(result.is_err(), "期望超时 Err，实际: {:?}", result);
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("192.168.1.2"), "Err 消息应含节点 host，实际: {}", msg);
    }

    #[tokio::test]
    async fn test_run_read_routing_phase_no_readonly() {
        let node = make_standby_node("192.168.1.2", false);
        let dminit = make_dminit();
        let runner = Arc::new(MockRunner::new_strict(vec![]));
        let runners: Runners = vec![(node, runner)];
        let specific = make_specific();
        let result = run_read_routing_phase(&specific, &runners, &dminit).await;
        assert!(result.is_ok(), "无 read_only 节点应跳过验证，实际: {:?}", result);
    }
}
