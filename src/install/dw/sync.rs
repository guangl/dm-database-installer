//! 备库数据同步：在 primary 上做一次 dmrman 全量备份，经控制机中转下载/上传，
//! 再在 standby 上 RESTORE + RECOVER，建立与主库一致的数据基线。

use anyhow::{Context, Result};
use futures::future::try_join_all;

use crate::config::DwClusterConfig;
use crate::config::dw::DwNode;
use crate::install::steps::service;
use crate::ssh::{CommandRunner, shell_quote};

use super::NodeRunner;

const SYNC_TAR_REMOTE_PATH: &str = "/tmp/dm_dw_init_backup.tar.gz";
const SYNC_BACKUPSET_DIR_NAME: &str = "DW_INIT_BACKUP";

/// 按官方"脱机备份、脱机还原"流程同步备库数据：在 primary 上用 dmrman 做一次全量备份，
/// 打包后经控制机中转下载再上传到每个待同步的 standby，最后在 standby 上 restore + recover。
/// 此时各节点的 dmserver 均尚未启动（dminit 刚完成），dmrman 直接操作离线数据文件。
/// 若没有待同步的 standby（如全部已同步过），直接跳过，不会触碰 primary。
pub(super) async fn sync_standbys_from_primary(
    cluster: &DwClusterConfig,
    all_pairs: &[NodeRunner<'_>],
    pending_standbys: &[NodeRunner<'_>],
) -> Result<()> {
    if pending_standbys.is_empty() {
        return Ok(());
    }
    let primary_node = cluster.primary();
    let (_, primary_runner) = all_pairs
        .iter()
        .find(|(node, _)| node.host == primary_node.host)
        .expect("primary_node 来自 cluster.nodes，必然能在 all_pairs 中找到");

    let primary_cfg = primary_node.as_install_config();
    let primary_dm_ini = service::dm_ini_path(&primary_cfg);
    let backup_dir = format!("{}/{}", primary_node.data_path, SYNC_BACKUPSET_DIR_NAME);
    let dmrman = format!("{}/bin/dmrman", primary_node.install_path);

    crate::ui::log_info(&format!("在 primary {} 上执行全量备份...", primary_node.host));
    let backup_cmd = format!(
        "su - dmdba -c {}",
        shell_quote(&format!(
            "{} CTLSTMT=\"BACKUP DATABASE '{}' FULL TO DW_INIT_BK BACKUPSET '{}'\"",
            dmrman, primary_dm_ini, backup_dir,
        ))
    );
    primary_runner
        .exec(&backup_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("primary 备份失败: {e}"))?;

    let tar_cmd = format!(
        "tar czf {} -C {} {}",
        shell_quote(SYNC_TAR_REMOTE_PATH),
        shell_quote(&primary_node.data_path),
        shell_quote(SYNC_BACKUPSET_DIR_NAME),
    );
    primary_runner
        .exec(&tar_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("打包备份集失败: {e}"))?;

    let tar_bytes = primary_runner
        .sftp_read(SYNC_TAR_REMOTE_PATH)
        .await
        .map_err(|e| anyhow::anyhow!("下载备份集失败: {e}"))?;
    let _ = primary_runner
        .exec(&format!("rm -f {}", shell_quote(SYNC_TAR_REMOTE_PATH)))
        .await;

    let futs = pending_standbys.iter().map(|(node, runner)| {
        let tar_bytes = tar_bytes.clone();
        async move {
            sync_one_standby(*runner, node, &tar_bytes)
                .await
                .with_context(|| format!("standby 节点 {} 数据同步失败", node.host))
        }
    });
    try_join_all(futs).await?;
    Ok(())
}

async fn sync_one_standby(runner: &dyn CommandRunner, node: &DwNode, tar_bytes: &[u8]) -> Result<()> {
    crate::ui::log_info(&format!("同步备份集到 standby {}...", node.host));
    runner
        .sftp_write(SYNC_TAR_REMOTE_PATH, tar_bytes)
        .await
        .map_err(|e| anyhow::anyhow!("上传备份集失败: {e}"))?;

    let untar_cmd = format!(
        "mkdir -p {data_path} && tar xzf {tar} -C {data_path} && rm -f {tar}",
        data_path = shell_quote(&node.data_path),
        tar = shell_quote(SYNC_TAR_REMOTE_PATH),
    );
    runner
        .exec(&untar_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("解包备份集失败: {e}"))?;

    let cfg = node.as_install_config();
    let dm_ini = service::dm_ini_path(&cfg);
    let backup_dir = format!("{}/{}", node.data_path, SYNC_BACKUPSET_DIR_NAME);
    let dmrman = format!("{}/bin/dmrman", node.install_path);

    let restore_cmd = format!(
        "su - dmdba -c {}",
        shell_quote(&format!(
            "{} CTLSTMT=\"RESTORE DATABASE '{}' FROM BACKUPSET '{}'\"",
            dmrman, dm_ini, backup_dir,
        ))
    );
    runner
        .exec(&restore_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("standby restore 失败: {e}"))?;

    let recover_cmd = format!(
        "su - dmdba -c {}",
        shell_quote(&format!(
            "{} CTLSTMT=\"RECOVER DATABASE '{}' UPDATE DB_MAGIC\"",
            dmrman, dm_ini,
        ))
    );
    runner
        .exec(&recover_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("standby recover 失败: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dw::NodeRole;
    use crate::install::dw::test_support::make_cluster;
    use crate::ssh::MockRunner;

    #[tokio::test]
    async fn test_sync_standbys_from_primary_runs_backup_then_restore_recover() {
        let cluster = make_cluster();
        let primary = MockRunner::new(vec![]);
        let standby = MockRunner::new(vec![]);
        primary.set_sftp_read(SYNC_TAR_REMOTE_PATH, b"fake-tar-bytes".to_vec());
        let all_pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&primary, &standby].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();
        let pending_standbys: Vec<NodeRunner> = all_pairs
            .iter()
            .filter(|(n, _)| n.role == NodeRole::Standby)
            .copied()
            .collect();

        sync_standbys_from_primary(&cluster, &all_pairs, &pending_standbys)
            .await
            .unwrap();

        let primary_log = primary.exec_log();
        assert!(
            primary_log.iter().any(|c| c.contains("BACKUP DATABASE")),
            "primary 应执行备份: {:?}",
            primary_log
        );
        assert!(
            primary_log.iter().any(|c| c.contains("tar czf")),
            "primary 应打包备份集: {:?}",
            primary_log
        );

        let standby_sftp = standby.sftp_log();
        assert!(
            standby_sftp
                .iter()
                .any(|(p, content)| p == SYNC_TAR_REMOTE_PATH && content == b"fake-tar-bytes"),
            "standby 应收到下载的备份集: {:?}",
            standby_sftp
        );
        let standby_log = standby.exec_log();
        assert!(
            standby_log.iter().any(|c| c.contains("RESTORE DATABASE")),
            "standby 应执行 restore: {:?}",
            standby_log
        );
        assert!(
            standby_log.iter().any(|c| c.contains("RECOVER DATABASE")),
            "standby 应执行 recover: {:?}",
            standby_log
        );
    }

    #[tokio::test]
    async fn test_sync_standbys_from_primary_skips_when_no_pending() {
        let cluster = make_cluster();
        let primary = MockRunner::new_strict(vec![]);
        let standby = MockRunner::new_strict(vec![]);
        let all_pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&primary, &standby].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();
        sync_standbys_from_primary(&cluster, &all_pairs, &[])
            .await
            .unwrap();
        assert!(primary.exec_log().is_empty(), "无待同步节点时不应触碰 primary");
    }
}
