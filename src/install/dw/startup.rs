//! Mount 模式启动、设置 OGUID/角色、dmwatcher/dmmonitor 服务注册启动。

use anyhow::{Context, Result};
use futures::future::try_join_all;

use crate::config::DwClusterConfig;
use crate::config::InstallConfig;
use crate::config::dw::DwNode;
use crate::install::steps::service;
use crate::ssh::{CommandRunner, shell_quote};

use super::NodeRunner;
use super::config_files;

/// 以 Mount 模式注册并启动 dmserver 服务（官方要求："一定要以 Mount 方式启动数据库实例"）。
/// 通过 `dm_service_installer.sh -t dmserver -m mount` 注册为 systemd 服务，
/// Mount 模式由服务定义固定下来——之后每次启动/重启都保持 Mount 模式，
/// 交由已注册并运行的 dmwatcher 服务负责将其切换为 Open 状态。
pub(super) async fn start_mount_mode(runner: &dyn CommandRunner, config: &InstallConfig) -> Result<()> {
    service::register_and_start_mount(runner, config)
        .await
        .with_context(|| format!("节点（实例 {}）mount 模式注册/启动失败", config.instance_name))?;
    service::wait_process_alive(runner, 60)
        .await
        .with_context(|| format!("dmserver（实例 {}）未在预期时间内启动", config.instance_name))
}

/// 通过 disql 设置 OGUID 并修改数据库角色（PRIMARY/STANDBY），按官方流程在修改前后
/// 临时打开/关闭 `ALTER_MODE_STATUS`。disql 顶层调用存储过程需要 `call` 前缀。
async fn set_oguid_and_role(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
    oguid: u32,
    role: &str,
) -> Result<()> {
    const SQL_PATH: &str = "/tmp/dm_dw_set_role.sql";
    let sql = format!(
        "call SP_SET_PARA_VALUE(1, 'ALTER_MODE_STATUS', 1);\n\
         call sp_set_oguid({oguid});\n\
         ALTER DATABASE {role};\n\
         call SP_SET_PARA_VALUE(1, 'ALTER_MODE_STATUS', 0);\n\
         exit;\n",
    );
    runner
        .sftp_write(SQL_PATH, sql.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("写入设置 OGUID/角色脚本失败: {e}"))?;

    let disql = format!("{}/bin/disql", config.install_path);
    let conn = format!("SYSDBA/{}@localhost:{}", sysdba_pwd, config.port);
    let cmd = crate::install::steps::disql_script_cmd(&disql, &conn, SQL_PATH);
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("设置 OGUID/角色失败: {e}"))?;

    let _ = runner.exec(&format!("rm -f {}", shell_quote(SQL_PATH))).await;
    Ok(())
}

/// primary 先 mount 启动并设置为 PRIMARY 角色，再并发处理所有待处理的 standby
/// （mount 启动 + 设置为 STANDBY 角色）。`pending` 为本次需要处理的节点子集
/// （断点续传：已完成的节点不会出现在其中）。
pub(super) async fn start_databases(
    cluster: &DwClusterConfig,
    pending: &[NodeRunner<'_>],
    sysdba_pwd: &str,
) -> Result<()> {
    let primary_node = cluster.primary();
    let primary_pending = pending.iter().find(|(node, _)| node.host == primary_node.host);
    if let Some((node, runner)) = primary_pending {
        let cfg = node.as_install_config();
        start_mount_mode(*runner, &cfg)
            .await
            .with_context(|| format!("primary 节点 {} mount 启动失败", node.host))?;
        set_oguid_and_role(*runner, &cfg, sysdba_pwd, cluster.oguid, "PRIMARY")
            .await
            .with_context(|| format!("primary 节点 {} 设置角色失败", node.host))?;
    }

    crate::ui::log_info(&format!(
        "正在处理集群中 {} 个 standby 节点（本次待处理 {} 个）...",
        cluster.standbys().count(),
        pending.len().saturating_sub(primary_pending.is_some() as usize)
    ));

    let standby_futs = pending
        .iter()
        .filter(|(node, _)| node.host != primary_node.host)
        .map(|(node, runner)| {
            let cfg = node.as_install_config();
            async move {
                start_mount_mode(*runner, &cfg)
                    .await
                    .with_context(|| format!("standby 节点 {} mount 启动失败", node.host))?;
                set_oguid_and_role(*runner, &cfg, sysdba_pwd, cluster.oguid, "STANDBY")
                    .await
                    .with_context(|| format!("standby 节点 {} 设置角色失败", node.host))
            }
        });
    try_join_all(standby_futs).await?;
    Ok(())
}

/// 在指定节点（通常是 primary）上注册并启动 dmmonitor 监视器服务。
/// 简化实现：监视器与某个数据库节点共置运行，未引入独立监视器主机。
pub(super) async fn start_monitor(
    cluster: &DwClusterConfig,
    node: &DwNode,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let monitor_path = format!("{}/dmmonitor.ini", node.data_path);
    runner
        .sftp_write(&monitor_path, config_files::dmmonitor_ini(cluster).as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("写入 dmmonitor.ini 失败: {e}"))?;

    service::register_and_start_monitor(runner, &node.install_path, &node.instance_name, &monitor_path)
        .await
        .with_context(|| format!("节点 {} 注册/启动 dmmonitor 服务失败", node.host))
}

/// 各节点注册并启动 dmwatcher 守护进程服务。
pub(super) async fn start_watchers_all(pairs: &[NodeRunner<'_>]) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        let watcher_ini = format!("{}/DAMENG/dmwatcher.ini", node.data_path);
        service::register_and_start_watcher(*runner, &node.install_path, &node.instance_name, &watcher_ini)
            .await
            .with_context(|| format!("节点 {} 注册/启动 dmwatcher 服务失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dw::NodeRole;
    use crate::install::dw::test_support::{make_cluster, make_node};
    use crate::ssh::MockRunner;

    fn root_runner(responses: Vec<(String, u32, Vec<u8>)>) -> MockRunner {
        let mut all = vec![("id -u".to_string(), 0, b"0\n".to_vec())];
        all.extend(responses);
        MockRunner::new(all)
    }

    #[tokio::test]
    async fn test_start_watchers_all_registers_dmwatcher_service() {
        let cluster = make_cluster();
        let m1 = root_runner(vec![]);
        let m2 = root_runner(vec![]);
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&m1, &m2].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();
        start_watchers_all(&pairs).await.unwrap();

        for m in [&m1, &m2] {
            let log = m.exec_log();
            assert!(
                log.iter().any(|c| c.contains("'dmwatcher'")),
                "应注册 dmwatcher 服务: {:?}",
                log
            );
        }
    }

    #[tokio::test]
    async fn test_start_mount_mode_registers_with_mount_flag() {
        let runner = root_runner(vec![("pgrep".to_string(), 0, b"alive\n".to_vec())]);
        let cfg = make_node(NodeRole::Primary, "h", "DM01").as_install_config();
        start_mount_mode(&runner, &cfg).await.unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|c| c.contains("'dmserver'") && c.contains("'-m'") && c.contains("'mount'")),
            "应以 -m mount 注册 dmserver 服务: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_set_oguid_and_role_writes_expected_sql() {
        let runner = MockRunner::new(vec![]);
        let cfg = make_node(NodeRole::Primary, "h", "DM01").as_install_config();
        set_oguid_and_role(&runner, &cfg, "Pwd123", 453331, "PRIMARY")
            .await
            .unwrap();
        let sftp_log = runner.sftp_log();
        let (_, content) = sftp_log
            .iter()
            .find(|(p, _)| p == "/tmp/dm_dw_set_role.sql")
            .expect("应写入设置角色脚本");
        let sql = String::from_utf8_lossy(content);
        assert!(sql.contains("call sp_set_oguid(453331)"));
        assert!(sql.contains("ALTER DATABASE PRIMARY"));
        assert!(sql.contains("call SP_SET_PARA_VALUE(1, 'ALTER_MODE_STATUS', 1)"));
        assert!(sql.contains("call SP_SET_PARA_VALUE(1, 'ALTER_MODE_STATUS', 0)"));
    }

    #[tokio::test]
    async fn test_start_monitor_registers_dmmonitor_service() {
        let cluster = make_cluster();
        let runner = root_runner(vec![]);
        let monitor_node = cluster.monitor_node().clone();
        start_monitor(&cluster, &monitor_node, &runner).await.unwrap();

        let sftp_log = runner.sftp_log();
        assert!(
            sftp_log.iter().any(|(p, _)| p.ends_with("dmmonitor.ini")),
            "应写入 dmmonitor.ini: {:?}",
            sftp_log
        );
        let exec_log = runner.exec_log();
        assert!(
            exec_log.iter().any(|c| c.contains("'dmmonitor'")),
            "应注册 dmmonitor 服务: {:?}",
            exec_log
        );
    }
}
