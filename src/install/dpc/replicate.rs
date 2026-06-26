//! 多副本（RAFT）数据同步：在每个 raft_group 的主副本（raft_self_id==1）上做 dmrman 全量备份，
//! 经控制机中转传给同组其余副本，再在副本上 RESTORE + RECOVER UPDATE DB_MAGIC。
//! 复用 dw/sync.rs 的 tar + sftp_read/sftp_write 中转模式。
//! 参见达梦 DPC 集群部署文档（多副本备份还原 + USE_AP）。

use anyhow::{Context, Result};
use futures::future::try_join_all;

use crate::config::dpc::{DpcClusterConfig, DpcNode};
use crate::install::steps::service;
use crate::ssh::{CommandRunner, shell_quote};

use super::NodeRunner;

const REPL_TAR_REMOTE_PATH: &str = "/tmp/dm_dpc_init_backup.tar.gz";
const REPL_BACKUPSET_DIR_NAME: &str = "DPC_INIT_BACKUP";

/// 多副本初始化同步：每个 raft_group 的非主副本从主副本拉取一致数据基线。
/// 单副本模式（无任何 raft_group）下直接返回，不触碰任何节点。
pub(super) async fn replicate_non_primary(
    cluster: &DpcClusterConfig,
    all_pairs: &[NodeRunner<'_>],
) -> Result<()> {
    if !cluster.is_multi_replica() {
        return Ok(());
    }

    for group in cluster.raft_groups() {
        replicate_one_group(cluster, all_pairs, &group)
            .await
            .with_context(|| format!("raft_group {} 副本同步失败", group))?;
    }
    Ok(())
}

async fn replicate_one_group(
    cluster: &DpcClusterConfig,
    all_pairs: &[NodeRunner<'_>],
    group: &str,
) -> Result<()> {
    let members = cluster.raft_group_members(group);
    let primary = members
        .iter()
        .find(|n| n.raft_self_id == Some(1))
        .copied()
        .with_context(|| format!("raft_group {} 缺少主副本（raft_self_id==1）", group))?;
    let peers: Vec<&DpcNode> = members
        .iter()
        .copied()
        .filter(|n| n.host != primary.host)
        .collect();
    if peers.is_empty() {
        return Ok(());
    }

    let (_, primary_runner) = all_pairs
        .iter()
        .find(|(n, _)| n.host == primary.host)
        .copied()
        .with_context(|| format!("主副本 {} 不在节点连接列表中", primary.host))?;

    let primary_cfg = primary.as_install_config();
    let primary_dm_ini = service::dm_ini_path(&primary_cfg);
    let backup_dir = format!("{}/{}", primary.data_path, REPL_BACKUPSET_DIR_NAME);
    let dmrman = format!("{}/bin/dmrman", primary.install_path);

    crate::ui::log_info(&format!(
        "在 raft_group {} 主副本 {} 上执行全量备份...",
        group, primary.host
    ));
    let backup_cmd = format!(
        "su - dmdba -c {}",
        shell_quote(&format!(
            "{} CTLSTMT=\"BACKUP DATABASE '{}' FULL TO BACKUP_01 BACKUPSET '{}'\" USE_AP=2",
            dmrman, primary_dm_ini, backup_dir,
        ))
    );
    primary_runner
        .exec(&backup_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("主副本备份失败: {e}"))?;

    let tar_cmd = format!(
        "tar czf {} -C {} {}",
        shell_quote(REPL_TAR_REMOTE_PATH),
        shell_quote(&primary.data_path),
        shell_quote(REPL_BACKUPSET_DIR_NAME),
    );
    primary_runner
        .exec(&tar_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("打包备份集失败: {e}"))?;

    let tar_bytes = primary_runner
        .sftp_read(REPL_TAR_REMOTE_PATH)
        .await
        .map_err(|e| anyhow::anyhow!("下载备份集失败: {e}"))?;
    let _ = primary_runner
        .exec(&format!("rm -f {}", shell_quote(REPL_TAR_REMOTE_PATH)))
        .await;

    let futs = peers.iter().map(|peer| {
        let tar_bytes = tar_bytes.clone();
        let (_, peer_runner) = all_pairs
            .iter()
            .find(|(n, _)| n.host == peer.host)
            .copied()
            .expect("peer 来自 cluster.nodes，必然能在 all_pairs 中找到");
        async move {
            restore_one_peer(peer_runner, peer, &tar_bytes)
                .await
                .with_context(|| format!("副本 {} 数据同步失败", peer.host))
        }
    });
    try_join_all(futs).await?;
    Ok(())
}

async fn restore_one_peer(runner: &dyn CommandRunner, node: &DpcNode, tar_bytes: &[u8]) -> Result<()> {
    crate::ui::log_info(&format!("同步备份集到副本 {}...", node.host));
    runner
        .sftp_write(REPL_TAR_REMOTE_PATH, tar_bytes)
        .await
        .map_err(|e| anyhow::anyhow!("上传备份集失败: {e}"))?;

    let untar_cmd = format!(
        "mkdir -p {data_path} && tar xzf {tar} -C {data_path} && rm -f {tar}",
        data_path = shell_quote(&node.data_path),
        tar = shell_quote(REPL_TAR_REMOTE_PATH),
    );
    runner
        .exec(&untar_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("解包备份集失败: {e}"))?;

    let cfg = node.as_install_config();
    let dm_ini = service::dm_ini_path(&cfg);
    let backup_dir = format!("{}/{}", node.data_path, REPL_BACKUPSET_DIR_NAME);
    let dmrman = format!("{}/bin/dmrman", node.install_path);

    let restore_cmd = format!(
        "su - dmdba -c {}",
        shell_quote(&format!(
            "{} CTLSTMT=\"RESTORE DATABASE '{}' FROM BACKUPSET '{}'\" USE_AP=2",
            dmrman, dm_ini, backup_dir,
        ))
    );
    runner
        .exec(&restore_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("副本 restore 失败: {e}"))?;

    let recover_cmd = format!(
        "su - dmdba -c {}",
        shell_quote(&format!(
            "{} CTLSTMT=\"RECOVER DATABASE '{}' UPDATE DB_MAGIC\" USE_AP=2",
            dmrman, dm_ini,
        ))
    );
    runner
        .exec(&recover_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("副本 recover 失败: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::dpc::test_support::{make_multi_replica_cluster, make_single_replica_cluster};
    use crate::ssh::MockRunner;

    #[tokio::test]
    async fn test_replicate_non_primary_noop_in_single_replica() {
        let cluster = make_single_replica_cluster();
        let mocks: Vec<MockRunner> = cluster
            .nodes
            .iter()
            .map(|_| MockRunner::new_strict(vec![]))
            .collect();
        let all_pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();
        replicate_non_primary(&cluster, &all_pairs).await.unwrap();
        for m in &mocks {
            assert!(m.exec_log().is_empty(), "单副本模式不应触碰任何节点");
        }
    }

    #[tokio::test]
    async fn test_replicate_non_primary_backup_then_restore_recover() {
        let cluster = make_multi_replica_cluster();
        let mocks: Vec<MockRunner> = cluster.nodes.iter().map(|_| MockRunner::new(vec![])).collect();
        // 主副本 BP01 提供 tar 内容
        let primary_idx = cluster.nodes.iter().position(|n| n.raft_self_id == Some(1)).unwrap();
        mocks[primary_idx].set_sftp_read(REPL_TAR_REMOTE_PATH, b"fake-tar".to_vec());

        let all_pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();
        replicate_non_primary(&cluster, &all_pairs).await.unwrap();

        let primary_log = mocks[primary_idx].exec_log();
        assert!(primary_log.iter().any(|c| c.contains("BACKUP DATABASE") && c.contains("USE_AP=2")));
        assert!(primary_log.iter().any(|c| c.contains("tar czf")));

        let peer_idx = cluster.nodes.iter().position(|n| n.raft_self_id == Some(2)).unwrap();
        let peer_sftp = mocks[peer_idx].sftp_log();
        assert!(peer_sftp.iter().any(|(p, c)| p == REPL_TAR_REMOTE_PATH && c == b"fake-tar"));
        let peer_log = mocks[peer_idx].exec_log();
        assert!(peer_log.iter().any(|c| c.contains("RESTORE DATABASE")));
        assert!(peer_log.iter().any(|c| c.contains("RECOVER DATABASE") && c.contains("UPDATE DB_MAGIC")));
    }
}
