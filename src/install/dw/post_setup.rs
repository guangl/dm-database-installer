//! 集群建立完成后的收尾步骤：备份作业 / SQL 日志 / 参数优化。

use anyhow::{Context, Result};
use futures::future::try_join_all;

use crate::install::steps::{backup, param_tune, service, sql_log};

use super::NodeRunner;

pub(super) async fn backup_all(pairs: &[NodeRunner<'_>], sysdba_pwd: &str) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        backup::configure_jobs(*runner, &node.as_install_config(), sysdba_pwd)
            .await
            .with_context(|| format!("节点 {} 配置备份作业失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
}

pub(super) async fn sql_log_all(pairs: &[NodeRunner<'_>], sysdba_pwd: &str) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        sql_log::enable(*runner, &node.as_install_config(), sysdba_pwd)
            .await
            .with_context(|| format!("节点 {} 开启 SQL 日志失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
}

/// 应用官方自动参数调整脚本后重启 dmserver 服务使其生效。dmserver 已在启动步骤中注册为
/// systemd 服务（且固定以 Mount 模式启动），重启会保持 Mount 模式，由 dmwatcher 重新促升。
pub(super) async fn param_tune_all(pairs: &[NodeRunner<'_>], sysdba_pwd: &str) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        let cfg = node.as_install_config();
        param_tune::apply(*runner, &cfg, sysdba_pwd)
            .await
            .with_context(|| format!("节点 {} 应用参数优化失败", node.host))?;
        service::restart_dmserver(*runner, &cfg)
            .await
            .with_context(|| format!("节点 {} 重启 dmserver 失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::dw::test_support::make_cluster;
    use crate::ssh::{CommandRunner, MockRunner};

    #[tokio::test]
    async fn test_backup_all_configures_each_node() {
        let cluster = make_cluster();
        let m1 = MockRunner::new(vec![]);
        let m2 = MockRunner::new(vec![]);
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&m1, &m2].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();
        backup_all(&pairs, "Pwd123").await.unwrap();
        for m in [&m1, &m2] {
            assert!(
                m.exec_log().iter().any(|c| c.contains("mkdir -p")),
                "应为每个节点创建备份目录"
            );
        }
    }

    #[tokio::test]
    async fn test_sql_log_all_enables_each_node() {
        let cluster = make_cluster();
        let m1 = MockRunner::new(vec![]);
        let m2 = MockRunner::new(vec![]);
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&m1, &m2].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();
        sql_log_all(&pairs, "Pwd123").await.unwrap();
        for m in [&m1, &m2] {
            let sftp_log = m.sftp_log();
            assert!(
                sftp_log
                    .iter()
                    .any(|(_, content)| String::from_utf8_lossy(content).contains("SVR_LOG")),
                "应为每个节点开启 SQL 日志"
            );
        }
    }

    #[tokio::test]
    async fn test_param_tune_all_restarts_dmserver_service() {
        let cluster = make_cluster();
        let m1 = MockRunner::new(vec![]);
        let m2 = MockRunner::new(vec![]);
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&m1, &m2].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();
        param_tune_all(&pairs, "Pwd123").await.unwrap();
        for (m, node) in [(&m1, &cluster.nodes[0]), (&m2, &cluster.nodes[1])] {
            let log = m.exec_log();
            let name = service::service_name(&node.as_install_config());
            assert!(
                log.iter().any(|c| c.contains("restart") && c.contains(&name)),
                "应重启 dmserver 服务 {}: {:?}",
                name,
                log
            );
        }
    }
}
