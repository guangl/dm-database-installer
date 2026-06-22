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
        .backup
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

    let sql = generate_backup_job_sql(backup_path, &config.backup);
    runner
        .sftp_write(JOB_SQL_PATH, sql.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("写入备份作业 SQL 失败: {e}"))?;

    let disql = format!("{}/bin/disql", config.install_path);
    let conn = format!("SYSDBA/{}@localhost:{}", sysdba_pwd, config.port);
    let cmd = super::disql_script_cmd(&disql, &conn, JOB_SQL_PATH);
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("配置备份作业失败: {e}"))?;

    let _ = runner
        .exec(&format!("rm -f {}", shell_quote(JOB_SQL_PATH)))
        .await;
    Ok(())
}

/// 生成备份作业 SQL：全备(bakup_ql) + [可选]增量备份并切全备(bakup_zl) + 过期清理(bak_clear，保留 retain_days 天)。
///
/// 全量备份调度由 `full_backup_interval_days` 决定：
/// - `1`：全量备份按天调度，不创建增量备份作业（每天都是全量，没有"非全量天"）
/// - `7`：全量按周调度（周六），增量按周调度（周日-周五），与自然周对齐
/// - 其他 N：全量按 N 天间隔调度，增量按天调度（两者重合的那天会同时执行，增量内容很少）
fn generate_backup_job_sql(backup_path: &str, backup: &crate::config::BackupConfig) -> String {
    let full_schedule = match backup.full_backup_interval_days {
        7 => format!(
            "call SP_ADD_JOB_SCHEDULE('bakup_ql', 'diaoduql', 1, 2, 1, 64, 0, '{time}', NULL, '2020-06-25 22:43:59', NULL, '');",
            time = backup.full_backup_time
        ),
        n => format!(
            "call SP_ADD_JOB_SCHEDULE('bakup_ql', 'diaoduql', 1, 1, {n}, 0, 0, '{time}', NULL, '2020-06-25 22:43:59', NULL, '');",
            n = n,
            time = backup.full_backup_time
        ),
    };

    let full_job = format!(
        r#"call SP_CREATE_JOB('bakup_ql',1,0,'',0,0,'',0,'');
call SP_JOB_CONFIG_START('bakup_ql');
call SP_ADD_JOB_STEP('bakup_ql', 'bak_ql', 6, '01000000{path}', 1, 2, 0, 0, NULL, 0);
{schedule}
call SP_JOB_CONFIG_COMMIT('bakup_ql');
"#,
        path = backup_path,
        schedule = full_schedule,
    );

    let incr_job = if backup.full_backup_interval_days == 1 {
        String::new()
    } else {
        let incr_schedule = match backup.full_backup_interval_days {
            7 => format!(
                "call SP_ADD_JOB_SCHEDULE('bakup_zl', 'diaodu_zl', 1, 2, 1, 63, 0, '{time}', NULL, '2020-06-21 11:15:00', NULL, '');",
                time = backup.incr_backup_time
            ),
            _ => format!(
                "call SP_ADD_JOB_SCHEDULE('bakup_zl', 'diaodu_zl', 1, 1, 1, 0, 0, '{time}', NULL, '2020-06-21 11:15:00', NULL, '');",
                time = backup.incr_backup_time
            ),
        };
        format!(
            r#"
call SP_CREATE_JOB('bakup_zl',1,0,'',0,0,'',0,'');
call SP_JOB_CONFIG_START('bakup_zl');
call SP_ADD_JOB_STEP('bakup_zl', 'bak_zl', 6, '11000000{path}|{path}', 1, 0, 2, 6, NULL, 0);
call SP_ADD_JOB_STEP('bakup_zl', 'switch_quanbei', 6, '01000000{path}', 1, 2, 0, 0, NULL, 0);
{schedule}
call SP_JOB_CONFIG_COMMIT('bakup_zl');
"#,
            path = backup_path,
            schedule = incr_schedule,
        )
    };

    format!(
        r#"SP_INIT_JOB_SYS(1);

{full_job}{incr_job}
call SP_CREATE_JOB('bak_clear',1,0,'',0,0,'',0,'每天删除{retain_days}天前的备份');
call SP_JOB_CONFIG_START('bak_clear');
call SP_ADD_JOB_STEP('bak_clear', 'del_bak', 0, 'SF_BAKSET_BACKUP_DIR_ADD(''DISK'',''{path}'');
CALL SP_DB_BAKSET_REMOVE_BATCH(''DISK'',SYSDATE-{retain_days});', 1, 2, 0, 0, NULL, 0);
call SP_ADD_JOB_SCHEDULE('bak_clear', 'diaodu_del', 1, 1, 1, 0, 0, '{clean_time}', NULL, '2020-06-25 22:54:03', NULL, '');
call SP_JOB_CONFIG_COMMIT('bak_clear');

-- 全量备份的调度周期可能晚于安装当天（如每周/每 N 天一次），首次全备完成前增量/集群依赖会缺基线；
-- 此处立即执行一次全量备份，确保安装当天即有可用全备基线。
BACKUP DATABASE BACKUPSET '{path}/FULL_INIT';
exit;
"#,
        full_job = full_job,
        incr_job = incr_job,
        path = backup_path,
        retain_days = backup.retain_days,
        clean_time = backup.clean_time,
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
            backup: crate::config::BackupConfig {
                backup_path: backup_path.map(str::to_string),
                ..Default::default()
            },
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
    fn test_generate_backup_job_sql_default_weekly_schedule() {
        let backup = crate::config::BackupConfig {
            retain_days: 30,
            ..Default::default()
        };
        let sql = generate_backup_job_sql("/data/dmbak", &backup);
        assert!(sql.contains("'01000000/data/dmbak'"));
        assert!(sql.contains("'11000000/data/dmbak|/data/dmbak'"));
        assert!(sql.contains("每天删除30天前的备份"));
        assert!(sql.contains("SYSDATE-30"));
        assert!(sql.contains("'2020-06-25 22:43:59'"));
        assert!(sql.contains(", 64, 0, '02:00:00'"), "全备应在周六 02:00 调度");
        assert!(sql.contains(", 63, 0, '02:00:00'"), "增量备份应在周日-周五 02:00 调度");
        assert!(sql.contains(", 0, 0, '05:00:00'"), "清理作业应在每天 05:00 调度");
        assert!(sql.contains("BACKUP DATABASE BACKUPSET '/data/dmbak/FULL_INIT';"));
    }

    #[test]
    fn test_generate_backup_job_sql_uses_configured_times() {
        let backup = crate::config::BackupConfig {
            full_backup_time: "03:30:00".to_string(),
            incr_backup_time: "04:15:00".to_string(),
            clean_time: "06:45:00".to_string(),
            ..Default::default()
        };
        let sql = generate_backup_job_sql("/data/dmbak", &backup);
        assert!(sql.contains(", 64, 0, '03:30:00'"), "全备时间应可配置");
        assert!(sql.contains(", 63, 0, '04:15:00'"), "增量备份时间应可配置");
        assert!(sql.contains(", 0, 0, '06:45:00'"), "清理时间应可配置");
    }

    #[test]
    fn test_generate_backup_job_sql_daily_full_only_skips_incremental() {
        let backup = crate::config::BackupConfig {
            full_backup_interval_days: 1,
            ..Default::default()
        };
        let sql = generate_backup_job_sql("/data/dmbak", &backup);
        assert!(
            sql.contains("call SP_CREATE_JOB('bakup_ql',"),
            "应创建全量备份作业"
        );
        assert!(
            !sql.contains("call SP_CREATE_JOB('bakup_zl',"),
            "每天全量时不应创建增量备份作业"
        );
        assert!(
            sql.contains(", 1, 1, 0, 0, '02:00:00'"),
            "全量应按天调度（间隔 1 天）"
        );
    }

    #[test]
    fn test_generate_backup_job_sql_interval_full_keeps_daily_incremental() {
        let backup = crate::config::BackupConfig {
            full_backup_interval_days: 3,
            ..Default::default()
        };
        let sql = generate_backup_job_sql("/data/dmbak", &backup);
        assert!(
            sql.contains("call SP_CREATE_JOB('bakup_zl',"),
            "全量间隔大于 1 天时应保留增量备份作业"
        );
        assert!(
            sql.contains(", 1, 1, 3, 0, 0, '02:00:00'"),
            "全量应按 3 天间隔调度"
        );
        assert!(
            sql.contains("call SP_ADD_JOB_SCHEDULE('bakup_zl', 'diaodu_zl', 1, 1, 1, 0, 0, '02:00:00'"),
            "增量应按天调度"
        );
    }
}
