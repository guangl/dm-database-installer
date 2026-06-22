use anyhow::Result;

use crate::config::InstallConfig;
use super::service;
use crate::ssh::{CommandRunner, shell_quote};

const PARAM_SQL_PATH: &str = "/tmp/dm_auto_para_adj.sql";

/// 官方"自动参数调整"脚本（exec_mode=0，按机器实际内存/CPU自动调整）。
/// 脚本注明需重启 dmserver 才能生效。
const PARAM_SQL: &str = include_str!("sql/auto_para_adj_dm8.sql");

/// 执行自动参数调整脚本并重启 dmserver 使其生效。
pub async fn apply_and_restart(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Result<()> {
    runner
        .sftp_write(PARAM_SQL_PATH, PARAM_SQL.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("写入参数调整 SQL 失败: {e}"))?;

    let disql = format!("{}/bin/disql", config.install_path);
    let conn = format!("SYSDBA/{}@localhost:{}", sysdba_pwd, config.port);
    let cmd = super::disql_script_cmd(&disql, &conn, PARAM_SQL_PATH);
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("执行参数调整脚本失败: {e}"))?;

    let _ = runner
        .exec(&format!("rm -f {}", shell_quote(PARAM_SQL_PATH)))
        .await;

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
