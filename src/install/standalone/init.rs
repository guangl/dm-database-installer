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

/// 创建归档目录、写入 dmarch.ini 并在 dm.ini 中启用 ARCH_INI=1。
pub async fn write_dmarch_ini(runner: &dyn CommandRunner, config: &InstallConfig) -> Result<()> {
    let arch_path = crate::config::resolve_arch_path(&config.archive, &config.data_path);
    runner
        .exec(&format!("mkdir -p {}", shell_quote(&arch_path)))
        .await
        .map_err(|e| anyhow::anyhow!("创建归档目录失败: {e}"))?;

    let content = generate_standalone_dmarch_ini(config);
    let dmarch_path = format!("{}/dmarch.ini", config.data_path);
    runner
        .sftp_write(&dmarch_path, content.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("写入 dmarch.ini 失败: {e}"))?;

    let dm_ini = format!("{}/DAMENG/dm.ini", config.data_path);
    let cmd = format!(
        "grep -q '^ARCH_INI' {f} \
         && sed -i 's/^ARCH_INI.*/ARCH_INI = 1/' {f} \
         || echo 'ARCH_INI = 1' >> {f}",
        f = shell_quote(&dm_ini),
    );
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("写入 dm.ini ARCH_INI 失败: {e}"))?;
    Ok(())
}

/// 生成单机 dmarch.ini 内容（仅本地归档，无 REALTIME 段）。
pub fn generate_standalone_dmarch_ini(config: &InstallConfig) -> String {
    let arch_path = crate::config::resolve_arch_path(&config.archive, &config.data_path);
    crate::config::format_local_arch_section(&arch_path, &config.archive)
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

    #[tokio::test]
    async fn test_write_dmarch_ini_creates_arch_dir() {
        let runner = MockRunner::new(vec![]);
        write_dmarch_ini(&runner, &make_config()).await.unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|cmd| cmd.contains("mkdir -p")),
            "应创建归档目录: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_write_dmarch_ini_sets_arch_ini() {
        let runner = MockRunner::new(vec![]);
        write_dmarch_ini(&runner, &make_config()).await.unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|cmd| cmd.contains("ARCH_INI")),
            "应写入 ARCH_INI=1: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_write_dmarch_ini_writes_dmarch_file() {
        let runner = MockRunner::new(vec![]);
        write_dmarch_ini(&runner, &make_config()).await.unwrap();
        let sftp_log = runner.sftp_log();
        assert!(
            sftp_log.iter().any(|(path, _)| path.contains("dmarch.ini")),
            "应写入 dmarch.ini: {:?}",
            sftp_log.iter().map(|(p, _)| p).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_generate_standalone_dmarch_ini_default_arch_path() {
        let config = InstallConfig::default();
        let ini = generate_standalone_dmarch_ini(&config);
        assert!(ini.contains("ARCH_TYPE = LOCAL"), "应含 LOCAL 归档段");
        assert!(
            ini.contains(&format!("{}/arch", config.data_path)),
            "默认归档路径应为 data_path/arch"
        );
        assert!(ini.contains("ARCH_FILE_SIZE = 128"), "默认文件大小应为 128");
        assert!(ini.contains("ARCH_HANG_FLAG = 0"), "单机默认不挂起");
    }

    #[test]
    fn test_generate_standalone_dmarch_ini_custom_arch_path() {
        use crate::config::ArchiveConfig;
        let config = InstallConfig {
            archive: ArchiveConfig {
                arch_path: Some("/data/myarch".to_string()),
                file_size: 256,
                space_limit: 2048,
                hang_flag: true,
                compressed: true,
            },
            ..Default::default()
        };
        let ini = generate_standalone_dmarch_ini(&config);
        assert!(
            ini.contains("ARCH_DEST = /data/myarch"),
            "应使用自定义归档路径"
        );
        assert!(
            ini.contains("ARCH_FILE_SIZE = 256"),
            "应使用自定义 file_size"
        );
        assert!(
            ini.contains("ARCH_SPACE_LIMIT = 2048"),
            "应使用自定义 space_limit"
        );
        assert!(
            ini.contains("ARCH_HANG_FLAG = 1"),
            "hang_flag=true 应输出 1"
        );
        assert!(
            ini.contains("ARCH_COMPRESSED = 1"),
            "compressed=true 应输出 1"
        );
    }
}
