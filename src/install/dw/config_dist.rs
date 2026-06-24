//! 分发 dmmal.ini / dmarch.ini / dmwatcher.ini 并修补 dm.ini 守护参数。

use anyhow::{Context, Result};
use futures::future::try_join_all;

use crate::config::DwClusterConfig;
use crate::config::dw::{ARCH_SPACE_LIMIT_DISK_PERCENT, ARCH_SPACE_LIMIT_FALLBACK_MB, DwNode};
use crate::install::steps::{preflight, service};
use crate::ssh::{CommandRunner, shell_quote};

use super::NodeRunner;
use super::config_files;

/// 在节点的数据目录下分发 dmmal.ini / dmarch.ini / dmwatcher.ini，并在 dm.ini 中
/// 补充 MAL_INI / ARCH_INI 引用（幂等：已存在引用则跳过）。三个 ini 文件统一放在
/// dm.ini 同级目录（{data_path}/DAMENG/），与 dm.ini 引用路径保持一致。
/// `dmmal.ini` 的节点列表始终来自完整 `cluster`（即使本次只分发给部分待处理节点）。
pub(super) async fn distribute_config_all(
    cluster: &DwClusterConfig,
    pairs: &[NodeRunner<'_>],
) -> Result<()> {
    let mal_ini = config_files::dmmal_ini(cluster);

    let futs = pairs.iter().map(|(node, runner)| {
        let mal_ini = mal_ini.clone();
        async move {
            let conf_dir = format!("{}/DAMENG", node.data_path);
            let mal_path = format!("{conf_dir}/dmmal.ini");
            let arch_path = format!("{conf_dir}/dmarch.ini");
            let watcher_path = format!("{conf_dir}/dmwatcher.ini");

            runner
                .sftp_write(&mal_path, mal_ini.as_bytes())
                .await
                .map_err(|e| anyhow::anyhow!("写入 dmmal.ini 失败: {e}"))?;
            let space_limit_mb =
                resolve_arch_space_limit_mb(*runner, cluster.arch.arch_space_limit, &node.resolve_arch_path()).await;
            runner
                .sftp_write(&arch_path, config_files::dmarch_ini(node, cluster, space_limit_mb).as_bytes())
                .await
                .map_err(|e| anyhow::anyhow!("写入 dmarch.ini 失败: {e}"))?;
            runner
                .sftp_write(
                    &watcher_path,
                    config_files::dmwatcher_ini(node, cluster).as_bytes(),
                )
                .await
                .map_err(|e| anyhow::anyhow!("写入 dmwatcher.ini 失败: {e}"))?;

            patch_dm_ini(*runner, node)
                .await
                .with_context(|| format!("节点 {} 更新 dm.ini 失败", node.host))
        }
    });
    try_join_all(futs).await?;
    Ok(())
}

/// 解析归档空间上限（MB）：显式配置则直接使用（0 = 无限）；未配置则查询归档目录所在
/// 磁盘总容量取其 20%，查询失败（如 SSH 报错/df 输出无法解析）时退回固定默认值，
/// 不让磁盘探测失败阻塞整个集群安装。
async fn resolve_arch_space_limit_mb(
    runner: &dyn CommandRunner,
    configured: Option<u32>,
    arch_path: &str,
) -> u32 {
    let Some(limit) = configured else {
        return match preflight::disk_total_bytes(runner, arch_path).await {
            Ok(total_bytes) => {
                let total_mb = total_bytes / (1024 * 1024);
                (total_mb * ARCH_SPACE_LIMIT_DISK_PERCENT / 100) as u32
            }
            Err(_) => ARCH_SPACE_LIMIT_FALLBACK_MB,
        };
    };
    limit
}

/// 幂等地在 dm.ini 中补充数据守护所需参数（已存在则跳过，不覆盖用户已有配置）。
/// `MAL_INI`/`ARCH_INI` 是布尔开关（=1 表示启用，对应文件按约定与 dm.ini 同目录同名查找），
/// 不是文件路径——这是与单机归档配置（`archive` 模块写绝对路径）的关键区别。
async fn patch_dm_ini(runner: &dyn CommandRunner, node: &DwNode) -> Result<()> {
    let dm_ini = shell_quote(&service::dm_ini_path(&node.as_install_config()));
    const PARAMS: &[(&str, &str)] = &[
        ("MAL_INI", "1"),
        ("ARCH_INI", "1"),
        ("DW_INACTIVE_INTERVAL", "60"),
        ("ENABLE_OFFLINE_TS", "2"),
        ("RLOG_SEND_APPLY_MON", "64"),
    ];
    let cmd: String = PARAMS
        .iter()
        .map(|(key, value)| {
            format!("grep -q '^{key}' {dm_ini} || echo '{key} = {value}' >> {dm_ini}; ")
        })
        .collect();
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("更新 dm.ini 失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::dw::test_support::make_cluster;
    use crate::ssh::MockRunner;

    #[tokio::test]
    async fn test_distribute_config_all_writes_three_ini_files_and_patches_dm_ini() {
        let cluster = make_cluster();
        let m1 = MockRunner::new(vec![]);
        let m2 = MockRunner::new(vec![]);
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&m1, &m2].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();
        distribute_config_all(&cluster, &pairs).await.unwrap();

        for m in [&m1, &m2] {
            let sftp_log = m.sftp_log();
            let paths: Vec<&String> = sftp_log.iter().map(|(p, _)| p).collect();
            assert!(paths.iter().any(|p| p.ends_with("dmmal.ini")));
            assert!(paths.iter().any(|p| p.ends_with("dmarch.ini")));
            assert!(paths.iter().any(|p| p.ends_with("dmwatcher.ini")));
            let exec_log = m.exec_log();
            assert!(
                exec_log.iter().any(|c| c.contains("MAL_INI")),
                "应在 dm.ini 中追加 MAL_INI 引用: {:?}",
                exec_log
            );
            assert!(
                exec_log.iter().any(|c| c.contains("ARCH_INI")),
                "应在 dm.ini 中追加 ARCH_INI 引用: {:?}",
                exec_log
            );
        }
    }

    #[tokio::test]
    async fn test_resolve_arch_space_limit_mb_uses_explicit_value() {
        let runner = MockRunner::new(vec![]);
        let limit = resolve_arch_space_limit_mb(&runner, Some(2048), "/data/arch").await;
        assert_eq!(limit, 2048);
    }

    #[tokio::test]
    async fn test_resolve_arch_space_limit_mb_uses_20_percent_of_disk() {
        // 100 GB 总容量 -> 20% = 20480 MB
        let df_output = b"Filesystem     1B-blocks      Used  Available Use% Mounted on\n\
/dev/sda1     107374182400 1000000 106374182400  1% /data\n"
            .to_vec();
        let runner = MockRunner::new(vec![("df -B1".to_string(), 0, df_output)]);
        let limit = resolve_arch_space_limit_mb(&runner, None, "/data/arch").await;
        assert_eq!(limit, 20480);
    }

    #[tokio::test]
    async fn test_resolve_arch_space_limit_mb_falls_back_when_disk_probe_fails() {
        let runner = MockRunner::new_strict(vec![]);
        let limit = resolve_arch_space_limit_mb(&runner, None, "/data/arch").await;
        assert_eq!(limit, ARCH_SPACE_LIMIT_FALLBACK_MB);
    }
}
