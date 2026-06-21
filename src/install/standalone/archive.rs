use anyhow::Result;

use crate::config::InstallConfig;
use crate::ssh::{CommandRunner, shell_quote};

const ARCH_SQL_PATH: &str = "/tmp/dm_enable_archive.sql";

/// 在线开启本地归档（MOUNT → ARCHIVELOG → ADD ARCHIVELOG → OPEN），无需重启 dmserver。
/// 必须在服务已注册并启动之后执行（需要数据库连接）。
pub async fn enable_archive_online(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Result<()> {
    let arch_path = crate::config::resolve_arch_path(&config.archive, &config.data_path);

    runner
        .exec(&format!("mkdir -p {}", shell_quote(&arch_path)))
        .await
        .map_err(|e| anyhow::anyhow!("创建归档目录失败: {e}"))?;

    let sql = generate_enable_archive_sql(&arch_path, &config.archive);
    runner
        .sftp_write(ARCH_SQL_PATH, sql.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("写入归档开启 SQL 失败: {e}"))?;

    let disql = format!("{}/bin/disql", config.install_path);
    let conn = format!("SYSDBA/{}@localhost:{}", sysdba_pwd, config.port);
    let inner_cmd = format!(
        "{} {} < {}",
        shell_quote(&disql),
        shell_quote(&conn),
        shell_quote(ARCH_SQL_PATH),
    );
    let cmd = format!("su - dmdba -c {}", shell_quote(&inner_cmd));
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("在线开启归档失败: {e}"))?;

    let _ = runner
        .exec(&format!("rm -f {}", shell_quote(ARCH_SQL_PATH)))
        .await;
    Ok(())
}

fn generate_enable_archive_sql(arch_path: &str, archive: &crate::config::ArchiveConfig) -> String {
    format!(
        "alter database mount;\n\
         alter database archivelog;\n\
         alter database add archivelog 'TYPE=LOCAL,DEST={path},FILE_SIZE={file_size},SPACE_LIMIT={space_limit}';\n\
         alter database open;\n\
         exit;\n",
        path = arch_path,
        file_size = archive.file_size,
        space_limit = archive.space_limit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockRunner;

    fn make_config(arch_path: Option<&str>) -> InstallConfig {
        InstallConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            port: 5236,
            archive: crate::config::ArchiveConfig {
                arch_path: arch_path.map(str::to_string),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_enable_archive_online_creates_arch_dir() {
        let runner = MockRunner::new(vec![]);
        enable_archive_online(&runner, &make_config(Some("/data/myarch")), "pwd1")
            .await
            .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter()
                .any(|cmd| cmd.contains("mkdir -p") && cmd.contains("/data/myarch")),
            "应创建归档目录: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_enable_archive_online_runs_disql_as_dmdba() {
        let runner = MockRunner::new(vec![]);
        enable_archive_online(&runner, &make_config(None), "pwd1")
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
    fn test_generate_enable_archive_sql_contains_mount_sequence() {
        let archive = crate::config::ArchiveConfig {
            arch_path: None,
            file_size: 256,
            space_limit: 2048,
        };
        let sql = generate_enable_archive_sql("/data/myarch", &archive);
        assert!(sql.contains("alter database mount;"));
        assert!(sql.contains("alter database archivelog;"));
        assert!(sql.contains(
            "alter database add archivelog 'TYPE=LOCAL,DEST=/data/myarch,FILE_SIZE=256,SPACE_LIMIT=2048';"
        ));
        assert!(sql.contains("alter database open;"));
    }
}
