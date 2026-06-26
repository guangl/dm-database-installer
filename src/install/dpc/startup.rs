//! 启动 SP / BP 节点（MP 已在 mp_bootstrap 中起且常驻）。
//! 启动顺序约束：MP → SP/BP；多副本时 BP/MP 以 MOUNT 模式启动等待 RAFT 接管。
//! 参见达梦 DPC 集群部署文档（启动顺序、MOUNT 模式）。

use anyhow::{Context, Result};
use futures::future::try_join_all;

use crate::config::dpc::{DpcClusterConfig, DpcNode, DpcRole};
use crate::install::steps::service;
use crate::ssh::{CommandRunner, shell_quote};

use super::NodeRunner;

/// 启动所有待处理的 SP / BP 节点。
/// - SP：`dmserver <dm.ini> dpc_mode=SP`
/// - BP：`dmserver <dm.ini> dpc_mode=BP`，多副本（该节点带 raft_group）时追加 ` MOUNT`
///
/// MP 节点不在此处理（已在 mp_bootstrap 启动）。
pub(super) async fn start_sp_bp_all(
    _cluster: &DpcClusterConfig,
    pending: &[NodeRunner<'_>],
) -> Result<()> {
    let futs = pending
        .iter()
        .filter(|(node, _)| matches!(node.role, DpcRole::Sp | DpcRole::Bp))
        .map(|(node, runner)| async move {
            start_one(*runner, node)
                .await
                .with_context(|| format!("节点 {} 启动失败", node.host))?;
            service::wait_process_alive(*runner, 60)
                .await
                .with_context(|| format!("节点 {} dmserver 未在预期时间内启动", node.host))
        });
    try_join_all(futs).await?;
    Ok(())
}

/// 以 dmdba 身份后台启动单个 SP/BP 节点的 dmserver。
async fn start_one(runner: &dyn CommandRunner, node: &DpcNode) -> Result<()> {
    let dm_ini = service::dm_ini_path(&node.as_install_config());
    let dmserver = format!("{}/bin/dmserver", node.install_path);
    let log = format!("{}/DAMENG/dmserver_dpc.log", node.data_path);

    // 多副本 BP 以 MOUNT 模式启动，等待 RAFT 选主/恢复后再 Open。
    let mount = if node.role == DpcRole::Bp && node.raft_group.is_some() {
        " MOUNT"
    } else {
        ""
    };
    let inner = format!(
        "nohup {} {} dpc_mode={}{} >{} 2>&1 &",
        shell_quote(&dmserver),
        shell_quote(&dm_ini),
        node.role.as_str(),
        mount,
        shell_quote(&log),
    );
    let cmd = format!("su - dmdba -c {}", shell_quote(&inner));
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("启动 dmserver 失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::dpc::test_support::{make_multi_replica_cluster, make_single_replica_cluster};
    use crate::ssh::MockRunner;

    #[tokio::test]
    async fn test_start_sp_bp_all_single_replica_no_mount() {
        let cluster = make_single_replica_cluster();
        let mocks: Vec<MockRunner> = cluster
            .nodes
            .iter()
            .map(|_| MockRunner::new(vec![("pgrep".to_string(), 0, b"alive\n".to_vec())]))
            .collect();
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();
        start_sp_bp_all(&cluster, &pairs).await.unwrap();

        for (i, node) in cluster.nodes.iter().enumerate() {
            let log = mocks[i].exec_log();
            match node.role {
                DpcRole::Sp => assert!(log.iter().any(|c| c.contains("dpc_mode=SP"))),
                DpcRole::Bp => {
                    assert!(log.iter().any(|c| c.contains("dpc_mode=BP")));
                    assert!(!log.iter().any(|c| c.contains("MOUNT")), "单副本不应 MOUNT");
                }
                DpcRole::Mp => assert!(
                    !log.iter().any(|c| c.contains("dpc_mode=")),
                    "MP 不应在此启动"
                ),
            }
        }
    }

    #[tokio::test]
    async fn test_start_sp_bp_all_multi_replica_bp_mounts() {
        let cluster = make_multi_replica_cluster();
        let mocks: Vec<MockRunner> = cluster
            .nodes
            .iter()
            .map(|_| MockRunner::new(vec![("pgrep".to_string(), 0, b"alive\n".to_vec())]))
            .collect();
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();
        start_sp_bp_all(&cluster, &pairs).await.unwrap();

        for (i, node) in cluster.nodes.iter().enumerate() {
            if node.role == DpcRole::Bp {
                let log = mocks[i].exec_log();
                assert!(
                    log.iter().any(|c| c.contains("dpc_mode=BP MOUNT")),
                    "多副本 BP 应带 MOUNT: {:?}",
                    log
                );
            }
        }
    }
}
