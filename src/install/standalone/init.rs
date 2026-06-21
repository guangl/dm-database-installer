use anyhow::Result;

use crate::config::InstallConfig;
use crate::ssh::{CommandRunner, shell_quote};

/// 以 dmdba 用户身份执行 dminit 初始化数据库实例。
pub async fn run_dminit(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
    sysauditor_pwd: &str,
) -> Result<()> {
    let dminit = format!("{}/bin/dminit", config.install_path);
    let inner_cmd = format!(
        "{} PATH={} DB_NAME=DAMENG INSTANCE_NAME={} PORT_NUM={} PAGE_SIZE={} EXTENT_SIZE={} CHARSET={} CASE_SENSITIVE={} SYSDBA_PWD={} SYSAUDITOR_PWD={}",
        shell_quote(&dminit),
        shell_quote(&config.data_path),
        shell_quote(&config.instance_name),
        config.port,
        config.page_size,
        config.extent_size,
        config.charset,
        if config.case_sensitive { "Y" } else { "N" },
        shell_quote(sysdba_pwd),
        shell_quote(sysauditor_pwd),
    );
    let cmd = format!("su - dmdba -c {}", shell_quote(&inner_cmd));
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("dminit 执行失败: {e}"))?;
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
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_run_dminit_uses_su_dmdba() {
        let runner = MockRunner::new(vec![]);
        run_dminit(&runner, &make_config(), "Sysdba1@Pass", "Audit2@Pass")
            .await
            .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|cmd| cmd.starts_with("su - dmdba -c")),
            "dminit 应以 dmdba 身份执行: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_run_dminit_contains_passwords() {
        let runner = MockRunner::new(vec![]);
        run_dminit(&runner, &make_config(), "Sysdba1@Pass", "Audit2@Pass")
            .await
            .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|cmd| cmd.contains("SYSDBA_PWD=")),
            "dminit 命令应含 SYSDBA_PWD: {:?}",
            log
        );
        assert!(
            log.iter().any(|cmd| cmd.contains("SYSAUDITOR_PWD=")),
            "dminit 命令应含 SYSAUDITOR_PWD: {:?}",
            log
        );
    }

}
