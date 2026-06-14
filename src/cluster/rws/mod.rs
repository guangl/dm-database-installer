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
    phases::run_read_routing_phase(&specific, &runners, &dminit).await?;
    tracing::info!("集群部署完成");
    Ok(())
}
