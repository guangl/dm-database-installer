use anyhow::Result;

use crate::config::InstallConfig;
use crate::ssh::CommandRunner;

const SQL_LOG_SQL_PATH: &str = "/tmp/dm_enable_sql_log.sql";

/// 开启 SQL 日志（SVR_LOG）：SP_SET_PARA_VALUE(1,'SVR_LOG',1) 同时写入 dm.ini 并立即生效，无需重启。
/// disql 顶层语句调用存储过程必须加 CALL，否则会被当作未知函数引用解析失败
/// （PL/SQL begin...end 块内才可省略 CALL，与 param_tune 中的脚本不同）。
pub async fn enable(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Result<()> {
    let sql = "call SP_SET_PARA_VALUE(1,'SVR_LOG',1);\nexit;\n";
    super::execute_sql_script(
        runner,
        config,
        sysdba_pwd,
        SQL_LOG_SQL_PATH,
        sql,
        "写入开启 SQL 日志脚本失败",
        "开启 SQL 日志失败",
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockRunner;

    fn make_config() -> InstallConfig {
        InstallConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            port: 5236,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_enable_runs_disql_as_dmdba() {
        let runner = MockRunner::new(vec![]);
        enable(&runner, &make_config(), "pwd1").await.unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter()
                .any(|cmd| cmd.starts_with("su - dmdba -c") && cmd.contains("disql")),
            "应以 dmdba 身份执行 disql: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_enable_writes_svr_log_statement() {
        let runner = MockRunner::new(vec![]);
        enable(&runner, &make_config(), "pwd1").await.unwrap();
        let sftp_log = runner.sftp_log();
        let (_, content) = sftp_log
            .iter()
            .find(|(p, _)| p == SQL_LOG_SQL_PATH)
            .expect("应写入开启 SQL 日志脚本");
        assert!(
            String::from_utf8_lossy(content).contains("call SP_SET_PARA_VALUE(1,'SVR_LOG',1)"),
            "脚本应包含开启 SVR_LOG 的语句"
        );
    }
}
