use anyhow::Result;

use crate::config::InstallConfig;
use super::service;
use crate::ssh::CommandRunner;

const PARAM_SQL_PATH: &str = "/tmp/dm_auto_para_adj.sql";

/// 官方"自动参数调整"脚本（exec_mode=0，按机器实际内存/CPU自动调整）。
/// 脚本注明需重启 dmserver 才能生效。
const PARAM_SQL: &str = include_str!("sql/auto_para_adj_dm8.sql");

/// 执行官方自动参数调整脚本（不重启，调用方决定如何使其生效——单机走 systemd 重启，
/// 集群（DW 主备）走 mount 模式进程重启，因此重启逻辑拆分到调用方）。
pub async fn apply(runner: &dyn CommandRunner, config: &InstallConfig, sysdba_pwd: &str) -> Result<()> {
    super::execute_sql_script(
        runner,
        config,
        sysdba_pwd,
        PARAM_SQL_PATH,
        PARAM_SQL,
        "写入参数调整 SQL 失败",
        "执行参数调整脚本失败",
    )
    .await
}

/// 执行自动参数调整脚本并重启 dmserver（systemd 服务）使其生效。仅用于单机安装；
/// 集群安装见 `install::dw` 中基于 mount 模式进程重启的等价逻辑。
pub async fn apply_and_restart(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Result<()> {
    apply(runner, config, sysdba_pwd).await?;
    crate::ui::log_info("参数调整需重启数据库才能生效，正在重启 dmserver...");
    service::restart_dmserver(runner, config).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockRunner;

    fn make_config() -> InstallConfig {
        InstallConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            instance_name: "DMSERVER".to_string(),
            port: 5236,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_apply_and_restart_runs_disql_as_dmdba() {
        let runner = MockRunner::new(vec![]);
        apply_and_restart(&runner, &make_config(), "pwd1")
            .await
            .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter()
                .any(|cmd| cmd.starts_with("su - dmdba -c") && cmd.contains("disql")),
            "应以 dmdba 身份执行 disql: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_apply_and_restart_restarts_dmserver() {
        let runner = MockRunner::new(vec![]);
        apply_and_restart(&runner, &make_config(), "pwd1")
            .await
            .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|cmd| cmd.contains("restart")
                && cmd.contains(&service::service_name(&make_config()))),
            "应重启 dmserver 服务: {:?}",
            log
        );
    }
}
