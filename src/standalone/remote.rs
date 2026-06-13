use anyhow::{Context, Result};
use std::path::Path;

use crate::cli::InstallArgs;
use crate::common::ssh::{CommandRunner, SshSession};
use crate::config::ssh::{SshCredentials, SshTarget};
use crate::config::{CommonConfig, InstallConfig};
use crate::standalone::silent_install::generate_install_xml;

use super::{fetch_package, prompt_passwords, verify_checksum};

/// 对 shell 参数进行单引号转义，防止命令注入。
fn shell_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "'\\''"))
}

pub async fn run(
    args: &InstallArgs,
    common: CommonConfig,
    specific: &InstallConfig,
    target: &SshTarget,
) -> Result<()> {
    tracing::info!("开始安装达梦数据库（单机 SSH 远程: {}）", target.host);

    let password = match &target.password {
        Some(p) => p.clone(),
        None => prompt_ssh_password(&target.user, &target.host)?,
    };
    let creds = SshCredentials {
        user: target.user.clone(),
        identity_file: None,
        password: Some(password),
    };

    let session = connect_with_retry(&target.host, target.ssh_port, &creds, target.max_retries, target.retry_interval_secs).await?;

    if check_remote_idempotent(specific, &session).await? {
        return Ok(());
    }

    let (sysdba_pwd, sysauditor_pwd) = prompt_passwords()?;

    let package = fetch_package(args, &common).await?;
    verify_checksum(args, &package.path)?;

    let extract_dir = crate::standalone::package::extract_dminstall_bin(&package.path)
        .context("提取 DMInstall.bin 失败")?;

    upload_and_install_remote(specific, extract_dir.path(), &session).await?;
    run_dminit_remote(specific, &sysdba_pwd, &sysauditor_pwd, &session).await?;

    tracing::info!("单机 SSH 远程安装完成");
    Ok(())
}

/// 带重试的 SSH 连接：最多尝试 `1 + max_retries` 次，每次失败等待 `retry_interval_secs` 秒。
async fn connect_with_retry(
    host: &str,
    port: u16,
    creds: &SshCredentials,
    max_retries: u32,
    retry_interval_secs: u64,
) -> Result<SshSession> {
    let mut last_err = None;
    for attempt in 0..=max_retries {
        if attempt > 0 {
            println!(
                "[SSH] 连接失败，{} 秒后重试（第 {}/{} 次）...",
                retry_interval_secs, attempt, max_retries
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(retry_interval_secs)).await;
        }
        match SshSession::connect(host, port, creds).await {
            Ok(session) => return Ok(session),
            Err(e) => {
                tracing::warn!("[SSH] 连接 {}:{} 失败: {}", host, port, e);
                last_err = Some(e);
            }
        }
    }
    Err(anyhow::anyhow!(
        "SSH 连接 {}:{} 失败，已重试 {} 次: {}",
        host,
        port,
        max_retries,
        last_err.unwrap()
    ))
}

fn prompt_ssh_password(user: &str, host: &str) -> Result<String> {
    rpassword::prompt_password(format!("SSH 密码 ({}@{}): ", user, host))
        .map_err(|e| anyhow::anyhow!("读取 SSH 密码失败: {e}"))
}

async fn check_remote_idempotent(
    config: &InstallConfig,
    runner: &dyn CommandRunner,
) -> Result<bool> {
    tracing::info!("[1/5] 远端幂等性检测");
    let dm_ini = format!("{}/dm.ini", config.install_path);
    let cmd = format!("test -f {} && echo exists || echo absent", shell_quote(&dm_ini));
    let (output, _) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("远端幂等检测失败: {e}"))?;
    let result = String::from_utf8_lossy(&output);
    if result.trim() == "exists" {
        println!("已检测到远端达梦实例 ({}/dm.ini)，跳过安装", config.install_path);
        return Ok(true);
    }
    Ok(false)
}

async fn upload_and_install_remote(
    config: &InstallConfig,
    extract_dir: &Path,
    runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("[3/5] 上传安装包并远端静默安装");

    let xml_file = generate_install_xml(config).context("生成 XML response file 失败")?;
    let xml_content =
        std::fs::read_to_string(xml_file.path()).context("读取 XML 临时文件失败")?;
    let remote_xml = "/tmp/dm_standalone_install.xml".to_string();
    runner
        .sftp_write(&remote_xml, xml_content.as_bytes())
        .await
        .context("SFTP 上传 XML response file 失败")?;

    let bin_path = extract_dir.join("DMInstall.bin");
    let bin_bytes =
        std::fs::read(&bin_path).with_context(|| format!("读取 DMInstall.bin 失败: {}", bin_path.display()))?;
    let remote_bin = "/tmp/dm_standalone_DMInstall.bin".to_string();
    runner.sftp_write(&remote_bin, &bin_bytes).await.context("SFTP 上传 DMInstall.bin 失败")?;

    runner
        .exec(&format!("chmod +x {}", shell_quote(&remote_bin)))
        .await
        .map_err(|e| anyhow::anyhow!("chmod DMInstall.bin 失败: {e}"))?;

    let install_cmd = format!(
        "{} -q {}",
        shell_quote(&remote_bin),
        shell_quote(&remote_xml)
    );
    runner
        .exec(&install_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("远端 DMInstall.bin 执行失败: {e}"))?;

    Ok(())
}

async fn run_dminit_remote(
    config: &InstallConfig,
    sysdba_pwd: &str,
    sysauditor_pwd: &str,
    runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("[4/5] 远端 dminit 初始化");
    let dminit = format!("{}/bin/dminit", config.install_path);
    let cmd = format!(
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
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("远端 dminit 执行失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ssh::MockRunner;

    fn make_config() -> InstallConfig {
        InstallConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            instance_name: "DMSERVER".to_string(),
            port: 5236,
            page_size: 32,
            charset: 1,
            case_sensitive: true,
            extent_size: 32,
            ssh_target: None,
        }
    }

    #[tokio::test]
    async fn test_remote_idempotent_detects_existing() {
        let runner = MockRunner::new(vec![("test -f".to_string(), 0, b"exists\n".to_vec())]);
        let config = make_config();
        let result = check_remote_idempotent(&config, &runner).await.unwrap();
        assert!(result, "exists 输出应触发幂等跳过");
    }

    #[tokio::test]
    async fn test_remote_idempotent_proceeds_when_absent() {
        let runner = MockRunner::new(vec![("test -f".to_string(), 0, b"absent\n".to_vec())]);
        let config = make_config();
        let result = check_remote_idempotent(&config, &runner).await.unwrap();
        assert!(!result, "absent 输出应允许继续安装");
    }

    #[tokio::test]
    async fn test_upload_and_install_uploads_xml_and_bin() {
        let runner = MockRunner::new(vec![]);
        let config = make_config();
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("DMInstall.bin"), b"fake_bin").unwrap();
        let _ = upload_and_install_remote(&config, tmp.path(), &runner).await;
        let sftp_log = runner.sftp_log();
        assert!(
            sftp_log.iter().any(|(p, _)| p.contains(".xml")),
            "应上传 XML: {:?}",
            sftp_log.iter().map(|(p, _)| p).collect::<Vec<_>>()
        );
        assert!(
            sftp_log.iter().any(|(p, _)| p.contains("DMInstall.bin")),
            "应上传 DMInstall.bin: {:?}",
            sftp_log.iter().map(|(p, _)| p).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn test_run_dminit_remote_command_contains_passwords() {
        let runner = MockRunner::new(vec![]);
        let config = make_config();
        run_dminit_remote(&config, "Sysdba1@Pass", "Audit2@Pass", &runner)
            .await
            .unwrap();
        let exec_log = runner.exec_log();
        assert!(
            exec_log.iter().any(|cmd| cmd.contains("SYSDBA_PWD=")),
            "dminit 命令应含 SYSDBA_PWD: {:?}",
            exec_log
        );
        assert!(
            exec_log.iter().any(|cmd| cmd.contains("SYSAUDITOR_PWD=")),
            "dminit 命令应含 SYSAUDITOR_PWD: {:?}",
            exec_log
        );
    }

    #[tokio::test]
    async fn test_run_dminit_remote_quotes_paths() {
        let runner = MockRunner::new(vec![]);
        let config = make_config();
        run_dminit_remote(&config, "Pwd1@Test1", "Pwd2@Test2", &runner)
            .await
            .unwrap();
        let exec_log = runner.exec_log();
        assert!(
            exec_log.iter().any(|cmd| cmd.contains("'/opt/dmdbms/bin/dminit'")),
            "dminit 路径应经 shell_quote 包裹: {:?}",
            exec_log
        );
    }
}
