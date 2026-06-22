use anyhow::Result;

use crate::config::InstallConfig;
use super::preflight;
use crate::ssh::{CommandRunner, shell_quote};

const ARCH_SQL_PATH: &str = "/tmp/dm_enable_archive.sql";

/// 归档空间上限未配置时，默认取磁盘总容量的百分比。
const DEFAULT_SPACE_LIMIT_DISK_PERCENT: u64 = 20;

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

    let space_limit = resolve_space_limit_mb(runner, &config.archive, &arch_path).await?;
    let sql = generate_enable_archive_sql(&arch_path, &config.archive, space_limit);
    runner
        .sftp_write(ARCH_SQL_PATH, sql.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("写入归档开启 SQL 失败: {e}"))?;

    let disql = format!("{}/bin/disql", config.install_path);
    let conn = format!("SYSDBA/{}@localhost:{}", sysdba_pwd, config.port);
    let cmd = super::disql_script_cmd(&disql, &conn, ARCH_SQL_PATH);
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("在线开启归档失败: {e}"))?;

    let _ = runner
        .exec(&format!("rm -f {}", shell_quote(ARCH_SQL_PATH)))
        .await;
    Ok(())
}

/// 解析归档空间上限（MB）：显式配置则直接使用（0 = 无限）；
/// 未配置则查询归档目录所在磁盘总容量，取其 20%。
async fn resolve_space_limit_mb(
    runner: &dyn CommandRunner,
    archive: &crate::config::ArchiveConfig,
    arch_path: &str,
) -> Result<u32> {
    if let Some(limit) = archive.space_limit {
        return Ok(limit);
    }
    let total_bytes = preflight::disk_total_bytes(runner, arch_path).await?;
    let total_mb = total_bytes / (1024 * 1024);
    let default_limit = total_mb * DEFAULT_SPACE_LIMIT_DISK_PERCENT / 100;
    Ok(default_limit as u32)
}

fn generate_enable_archive_sql(
    arch_path: &str,
    archive: &crate::config::ArchiveConfig,
    space_limit: u32,
) -> String {
    format!(
        "alter database mount;\n\
         alter database archivelog;\n\
         alter database add archivelog 'TYPE=LOCAL,DEST={path},FILE_SIZE={file_size},SPACE_LIMIT={space_limit}';\n\
         alter database open;\n\
         exit;\n",
        path = arch_path,
        file_size = archive.file_size,
        space_limit = space_limit,
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
                space_limit: Some(0),
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
            space_limit: Some(2048),
        };
        let sql = generate_enable_archive_sql("/data/myarch", &archive, 2048);
        assert!(sql.contains("alter database mount;"));
        assert!(sql.contains("alter database archivelog;"));
        assert!(sql.contains(
            "alter database add archivelog 'TYPE=LOCAL,DEST=/data/myarch,FILE_SIZE=256,SPACE_LIMIT=2048';"
        ));
        assert!(sql.contains("alter database open;"));
    }

    #[tokio::test]
    async fn test_resolve_space_limit_mb_uses_explicit_value() {
        let runner = MockRunner::new(vec![]);
        let archive = crate::config::ArchiveConfig {
            space_limit: Some(4096),
            ..Default::default()
        };
        let limit = resolve_space_limit_mb(&runner, &archive, "/data/arch")
            .await
            .unwrap();
        assert_eq!(limit, 4096);
    }

    #[tokio::test]
    async fn test_resolve_space_limit_mb_defaults_to_20_percent_of_disk() {
        // 100 GB 总容量 -> 默认空间上限应为 20 GB = 20480 MB
        let df_output = b"Filesystem     1B-blocks      Used  Available Use% Mounted on\n\
/dev/sda1     107374182400 1000000 106374182400  1% /data\n"
            .to_vec();
        let runner = MockRunner::new(vec![("df -B1".to_string(), 0, df_output)]);
        let archive = crate::config::ArchiveConfig {
            space_limit: None,
            ..Default::default()
        };
        let limit = resolve_space_limit_mb(&runner, &archive, "/data/arch")
            .await
            .unwrap();
        assert_eq!(limit, 20480);
    }
}
