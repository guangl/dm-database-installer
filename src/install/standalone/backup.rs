use anyhow::Result;

use crate::config::InstallConfig;
use crate::ssh::{CommandRunner, shell_quote};

const JOB_SQL_PATH: &str = "/tmp/dm_backup_jobs.sql";

/// 在达梦作业系统中创建全备/增量备份/清理作业，写入 backup_path。
/// SQL 模板由用户提供，仅替换备份路径与保留天数，其余作业系统内部参数保持原样。
pub async fn configure_jobs(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Result<()> {
    let backup_path = config
        .backup_path
        .as_deref()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| anyhow::anyhow!("backup_path 未配置，无法创建备份作业"))?;

    if crate::install::advisory::path_overlaps(backup_path, &config.data_path) {
        crate::ui::log_warn(&format!(
            "备份目录与数据目录位于同一路径（{} ⊂/= {}），建议改为独立磁盘或目录，避免同盘故障导致数据与备份同时丢失",
            backup_path, config.data_path
        ));
    }

    runner
        .exec(&format!("mkdir -p {}", shell_quote(backup_path)))
        .await
        .map_err(|e| anyhow::anyhow!("创建备份目录失败: {e}"))?;

    let sql = generate_backup_job_sql(backup_path, config.backup.retain_days);
    runner
        .sftp_write(JOB_SQL_PATH, sql.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("写入备份作业 SQL 失败: {e}"))?;

    let disql = format!("{}/bin/disql", config.install_path);
    let conn = format!("SYSDBA/{}@localhost:{}", sysdba_pwd, config.port);
    let inner_cmd = format!(
        "{} {} < {}",
        shell_quote(&disql),
        shell_quote(&conn),
        shell_quote(JOB_SQL_PATH),
    );
    let cmd = format!("su - dmdba -c {}", shell_quote(&inner_cmd));
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("配置备份作业失败: {e}"))?;

    let _ = runner
        .exec(&format!("rm -f {}", shell_quote(JOB_SQL_PATH)))
        .await;
    Ok(())
}

/// 生成备份作业 SQL：全备(bakup_ql) + 增量备份并切全备(bakup_zl) + 过期清理(bak_clear)。
fn generate_backup_job_sql(backup_path: &str, retain_days: u32) -> String {
    format!(
        r#"SP_INIT_JOB_SYS(1);

call SP_CREATE_JOB('bakup_ql',1,0,'',0,0,'',0,'');
call SP_JOB_CONFIG_START('bakup_ql');
call SP_ADD_JOB_STEP('bakup_ql', 'bak_ql', 6, '01000000{path}', 1, 2, 0, 0, NULL, 0);
call SP_ADD_JOB_SCHEDULE('bakup_ql', 'diaoduql', 1, 2, 1, 64, 0, '01:00:00', NULL, '2020-06-25 22:43:59', NULL, '');
call SP_JOB_CONFIG_COMMIT('bakup_ql');

call SP_CREATE_JOB('bakup_zl',1,0,'',0,0,'',0,'');
call SP_JOB_CONFIG_START('bakup_zl');
call SP_ADD_JOB_STEP('bakup_zl', 'bak_zl', 6, '11000000{path}|{path}', 1, 0, 2, 6, NULL, 0);
call SP_ADD_JOB_STEP('bakup_zl', 'switch_quanbei', 6, '01000000{path}', 1, 2, 0, 0, NULL, 0);
call SP_ADD_JOB_SCHEDULE('bakup_zl', 'diaodu_zl', 1, 2, 1, 63, 0, '22:30:00', NULL, '2020-06-21 11:15:00', NULL, '');
call SP_JOB_CONFIG_COMMIT('bakup_zl');

call SP_CREATE_JOB('bak_clear',1,0,'',0,0,'',0,'每天删除{retain_days}天前的备份');
call SP_JOB_CONFIG_START('bak_clear');
call SP_ADD_JOB_STEP('bak_clear', 'del_bak', 0, 'SF_BAKSET_BACKUP_DIR_ADD(''DISK'',''{path}'');
CALL SP_DB_BAKSET_REMOVE_BATCH(''DISK'',SYSDATE-{retain_days});', 1, 2, 0, 0, NULL, 0);
call SP_ADD_JOB_SCHEDULE('bak_clear', 'diaodu_del', 1, 1, 1, 0, 0, '01:00:00', NULL, '2020-06-25 22:54:03', NULL, '');
call SP_JOB_CONFIG_COMMIT('bak_clear');
exit;
"#,
        path = backup_path,
        retain_days = retain_days,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockRunner;

    fn make_config(backup_path: Option<&str>, data_path: &str) -> InstallConfig {
        InstallConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: data_path.to_string(),
            backup_path: backup_path.map(str::to_string),
            port: 5236,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_configure_jobs_fails_without_backup_path() {
        let runner = MockRunner::new(vec![]);
        let err = configure_jobs(&runner, &make_config(None, "/opt/dmdbms/data"), "pwd1")
            .await
            .unwrap_err();
        assert!(
            format!("{err}").contains("backup_path 未配置"),
            "应提示 backup_path 未配置: {err}"
        );
    }

    #[tokio::test]
    async fn test_configure_jobs_creates_backup_dir() {
        let runner = MockRunner::new(vec![]);
        configure_jobs(
            &runner,
            &make_config(Some("/data/dmbak"), "/opt/dmdbms/data"),
            "pwd1",
        )
        .await
        .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|cmd| cmd.contains("mkdir -p")),
            "应创建备份目录: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_configure_jobs_runs_disql_as_dmdba() {
        let runner = MockRunner::new(vec![]);
        configure_jobs(
            &runner,
            &make_config(Some("/data/dmbak"), "/opt/dmdbms/data"),
            "pwd1",
        )
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

    #[test]
    fn test_generate_backup_job_sql_substitutes_path_and_retain_days() {
        let sql = generate_backup_job_sql("/data/dmbak", 30);
        assert!(sql.contains("'01000000/data/dmbak'"));
        assert!(sql.contains("'11000000/data/dmbak|/data/dmbak'"));
        assert!(sql.contains("每天删除30天前的备份"));
        assert!(sql.contains("SYSDATE-30"));
        // 不应影响其余不该被替换的字面值
        assert!(sql.contains("'2020-06-25 22:43:59'"));
        assert!(sql.contains(", 64, 0, '01:00:00'"));
    }
}
