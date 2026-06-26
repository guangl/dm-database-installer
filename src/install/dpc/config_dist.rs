//! 分发 DPC 配置文件：mp.ini（所有节点）与多副本 dmarch.ini（BP/MP 节点）。

use anyhow::{Context, Result};
use futures::future::try_join_all;

use crate::config::dpc::{DpcClusterConfig, DpcNode, DpcRole};

use super::NodeRunner;
use super::config_files;

/// 在所有节点的数据目录（{data_path}/DAMENG/）下写入 mp.ini，指向 MP 元数据节点。
pub(super) async fn distribute_mp_ini_all(
    pairs: &[NodeRunner<'_>],
    cluster: &DpcClusterConfig,
) -> Result<()> {
    let mp_ini = config_files::mp_ini(cluster);
    let futs = pairs.iter().map(|(node, runner)| {
        let mp_ini = mp_ini.clone();
        async move {
            let path = format!("{}/DAMENG/mp.ini", node.data_path);
            runner
                .sftp_write(&path, mp_ini.as_bytes())
                .await
                .map_err(|e| anyhow::anyhow!("写入 mp.ini 失败: {e}"))
                .with_context(|| format!("节点 {} 分发 mp.ini 失败", node.host))
        }
    });
    try_join_all(futs).await?;
    Ok(())
}

/// 多副本模式下，给每个 BP/MP 节点写入含 RAFT 归档段的 dmarch.ini。
/// 单副本模式（无任何 raft_group）下为 no-op。
pub(super) async fn distribute_arch_ini_raft(
    cluster: &DpcClusterConfig,
    pairs: &[NodeRunner<'_>],
) -> Result<()> {
    if !cluster.is_multi_replica() {
        return Ok(());
    }

    let futs = pairs
        .iter()
        // RAFT 归档仅对参与 raft_group 的 BP/MP 节点有意义。
        .filter(|(node, _)| {
            node.raft_group.is_some() && matches!(node.role, DpcRole::Bp | DpcRole::Mp)
        })
        .map(|(node, runner)| async move {
            let group = node
                .raft_group
                .as_deref()
                .expect("filter 已保证 raft_group.is_some()");
            // 同组内除本节点外的其他副本作为 RAFT 归档对端。
            let peers: Vec<&DpcNode> = cluster
                .raft_group_members(group)
                .into_iter()
                .filter(|n| n.host != node.host)
                .collect();
            let ini = config_files::dmarch_ini_raft(node, &peers, cluster);
            let path = format!("{}/DAMENG/dmarch.ini", node.data_path);
            runner
                .sftp_write(&path, ini.as_bytes())
                .await
                .map_err(|e| anyhow::anyhow!("写入 dmarch.ini 失败: {e}"))
                .with_context(|| format!("节点 {} 分发 dmarch.ini 失败", node.host))
        });
    try_join_all(futs).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::dpc::test_support::{make_multi_replica_cluster, make_single_replica_cluster};
    use crate::ssh::{CommandRunner, MockRunner};

    #[tokio::test]
    async fn test_distribute_mp_ini_all_writes_to_every_node() {
        let cluster = make_single_replica_cluster();
        let mocks: Vec<MockRunner> = cluster.nodes.iter().map(|_| MockRunner::new(vec![])).collect();
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();
        distribute_mp_ini_all(&pairs, &cluster).await.unwrap();
        for m in &mocks {
            assert!(m.sftp_log().iter().any(|(p, _)| p.ends_with("mp.ini")));
        }
    }

    #[tokio::test]
    async fn test_distribute_arch_ini_raft_noop_in_single_replica() {
        let cluster = make_single_replica_cluster();
        let mocks: Vec<MockRunner> = cluster.nodes.iter().map(|_| MockRunner::new(vec![])).collect();
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();
        distribute_arch_ini_raft(&cluster, &pairs).await.unwrap();
        for m in &mocks {
            assert!(m.sftp_log().is_empty(), "单副本不应写 dmarch.ini");
        }
    }

    #[tokio::test]
    async fn test_distribute_arch_ini_raft_writes_for_bp_nodes() {
        let cluster = make_multi_replica_cluster();
        let mocks: Vec<MockRunner> = cluster.nodes.iter().map(|_| MockRunner::new(vec![])).collect();
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();
        distribute_arch_ini_raft(&cluster, &pairs).await.unwrap();

        // 两个 BP raft 节点应写 dmarch.ini；SP 节点不写。
        for (i, node) in cluster.nodes.iter().enumerate() {
            let wrote = mocks[i].sftp_log().iter().any(|(p, _)| p.ends_with("dmarch.ini"));
            if node.raft_group.is_some() {
                assert!(wrote, "raft 节点 {} 应写 dmarch.ini", node.host);
            } else {
                assert!(!wrote, "非 raft 节点 {} 不应写 dmarch.ini", node.host);
            }
        }
    }
}
