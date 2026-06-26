//! 单机 SSH 远程安装与集群安装共用的远端操作 helper。
//! 从 `standalone::remote` 提升而来，保持行为不变，仅扩大可见性供 `cluster` 模块复用。

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::time::Duration;

use crate::config::InstallConfig;
use crate::config::ssh::SshCredentials;
use crate::platform::detect_platform_from_raw;
use crate::ssh::{CommandRunner, SshSession, shell_quote};

pub const REMOTE_BIN: &str = "/tmp/dm_remote_DMInstall.bin";
pub const REMOTE_XML: &str = "/tmp/dm_remote_install.xml";

pub async fn connect_with_retry(
    host: &str,
    port: u16,
    creds: &SshCredentials,
    max_retries: u32,
    retry_interval_secs: u64,
) -> Result<SshSession> {
    let mut last_err = None;
    for attempt in 0..=max_retries {
        if attempt > 0 {
            crate::ui::log_warn(&format!(
                "[SSH] 连接失败，{} 秒后重试（第 {}/{} 次）...",
                retry_interval_secs, attempt, max_retries
            ));
            tokio::time::sleep(tokio::time::Duration::from_secs(retry_interval_secs)).await;
        }
        match SshSession::connect(host, port, creds).await {
            Ok(session) => return Ok(session),
            Err(e) => {
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

pub fn prompt_ssh_password(user: &str, host: &str) -> Result<String> {
    rpassword::prompt_password(format!("SSH 密码 ({}@{}): ", user, host))
        .map_err(|e| anyhow::anyhow!("读取 SSH 密码失败: {e}"))
}

pub async fn exec_remote_str(runner: &dyn CommandRunner, cmd: &str) -> String {
    runner
        .exec(cmd)
        .await
        .map(|(bytes, _)| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

pub async fn detect_remote_language(runner: &dyn CommandRunner) -> &'static str {
    let lang = exec_remote_str(runner, "echo $LANG").await;
    if lang.trim().to_lowercase().contains("zh") {
        "ZH"
    } else {
        "EN"
    }
}

pub async fn detect_remote_platform(runner: &dyn CommandRunner) -> crate::platform::Platform {
    let uname = exec_remote_str(runner, "uname -m").await;
    let cpuinfo = exec_remote_str(runner, "cat /proc/cpuinfo 2>/dev/null || true").await;
    let os_release = exec_remote_str(runner, "cat /etc/os-release 2>/dev/null || true").await;
    detect_platform_from_raw(&uname, &cpuinfo, &os_release)
}

/// 执行 `test_expr && echo {true_marker} || echo {false_marker}`，返回结果是否命中 true_marker。
/// 三个远端存在性检测（数据目录占用/dmserver 已装/dminit 已完成）共用此 shape。
async fn check_remote_condition(
    runner: &dyn CommandRunner,
    test_expr: &str,
    true_marker: &str,
    false_marker: &str,
    err_ctx: &str,
) -> Result<bool> {
    let cmd = format!("{test_expr} && echo {true_marker} || echo {false_marker}");
    let (output, _) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("{err_ctx}: {e}"))?;
    Ok(String::from_utf8_lossy(&output).trim() == true_marker)
}

/// 检测远端 data_path 目录是否非空（说明有旧数据，dminit 会冲突）。
pub async fn check_remote_data_path_occupied(
    config: &InstallConfig,
    runner: &dyn CommandRunner,
) -> Result<bool> {
    let path = shell_quote(&config.data_path);
    let test_expr = format!("[ -d {path} ] && [ -n \"$(ls -A {path} 2>/dev/null)\" ]");
    check_remote_condition(runner, &test_expr, "occupied", "clean", "远端数据目录检测失败").await
}

/// 检测远端 install_path/bin/dmserver 是否已存在（说明有旧安装）。
pub async fn check_remote_dmserver_exists(
    config: &InstallConfig,
    runner: &dyn CommandRunner,
) -> Result<bool> {
    let dmserver = format!("{}/bin/dmserver", config.install_path);
    let test_expr = format!("test -f {}", shell_quote(&dmserver));
    check_remote_condition(runner, &test_expr, "exists", "absent", "远端 dmserver 检测失败").await
}

/// 检测远端 dminit 是否已完成：{data_path}/DAMENG/dm.ini 存在则认为已完成。
pub async fn check_remote_dminit_done(
    config: &InstallConfig,
    runner: &dyn CommandRunner,
) -> Result<bool> {
    let dm_ini = super::steps::service::dm_ini_path(config);
    let test_expr = format!("test -f {}", shell_quote(&dm_ini));
    check_remote_condition(runner, &test_expr, "exists", "absent", "远端幂等检测失败").await
}

/// 根据包类型上传 DMInstall.bin 到远端。
/// - 若本地包本身就是 DMInstall.bin → 直接上传
/// - 否则（ISO/其他）→ 本地提取 DMInstall.bin 后再上传，不依赖远端 mount
pub async fn upload_and_extract_on_remote(
    package_path: &Path,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let name = package_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if name.ends_with("DMInstall.bin") {
        return upload_bin(package_path, runner).await;
    }
    crate::ui::log_info("本地提取 DMInstall.bin...");
    let extract_dir = crate::install::steps::package::extract_dminstall_bin(package_path)
        .context("本地提取 DMInstall.bin 失败")?;
    let bin_path = extract_dir.path().join("DMInstall.bin");
    upload_bin(&bin_path, runner).await
}

/// 上传 DMInstall.bin 到远端 /tmp。
pub async fn upload_bin(bin_path: &Path, runner: &dyn CommandRunner) -> Result<()> {
    let bin_bytes = std::fs::read(bin_path)
        .with_context(|| format!("读取 DMInstall.bin 失败: {}", bin_path.display()))?;
    let pb = upload_progress_bar(bin_bytes.len() as u64);
    runner
        .sftp_write_with_progress(REMOTE_BIN, &bin_bytes, &|n| pb.inc(n))
        .await
        .context("SFTP 上传 DMInstall.bin 失败")?;
    pb.finish_with_message("上传完成");
    runner
        .sftp_set_permissions(REMOTE_BIN, 0o755)
        .await
        .context("设置 DMInstall.bin 执行权限失败")?;
    Ok(())
}

/// 上传安装 XML 并执行静默安装；成功后远端自动清理临时文件。
pub async fn run_remote_install(config: &InstallConfig, runner: &dyn CommandRunner) -> Result<()> {
    let language = detect_remote_language(runner).await;
    let xml = install_only_xml(&config.install_path, language);
    runner
        .sftp_write(REMOTE_XML, xml.as_bytes())
        .await
        .context("SFTP 上传安装 XML 失败")?;

    let spinner = install_spinner();
    const REMOTE_LOG: &str = "/tmp/dm_remote_install.log";
    let cmd = format!(
        "DISPLAY= {bin} -q {xml} > {log} 2>&1; \
         ret=$?; \
         [ $ret -eq 0 ] && rm -f {xml} {bin} {log}; \
         [ $ret -ne 0 ] && sed 's/\\x1b\\[[0-9;?]*[A-Za-z]//g; s/\\r//g' {log} | grep -v '^[[:space:]]*$'; \
         exit $ret",
        bin = shell_quote(REMOTE_BIN),
        xml = shell_quote(REMOTE_XML),
        log = REMOTE_LOG,
    );
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("远端安装 dmdbms 失败: {e}"))?;
    spinner.finish_with_message("安装完成");
    Ok(())
}

pub async fn check_remote_prerequisites(
    specific: &InstallConfig,
    runner: &dyn CommandRunner,
    skip_port_check: bool,
) -> Result<()> {
    use crate::install::steps::preflight::{
        check_cpu_cores, check_disk_space, check_memory, check_port_available, check_selinux,
        check_ulimits,
    };
    check_memory(runner).await.context("远端节点内存检测失败")?;
    check_cpu_cores(runner)
        .await
        .context("远端节点 CPU 检测失败")?;
    check_disk_space(runner, &specific.install_path)
        .await
        .context("远端节点磁盘检测失败")?;
    if !skip_port_check {
        check_port_available(runner, specific.port)
            .await
            .context("远端节点端口检测失败")?;
        check_port_available(runner, crate::config::AP_PORT_PRECHECK)
            .await
            .context("远端节点 AP 端口检测失败")?;
    }
    check_ulimits(runner)
        .await
        .context("远端节点 ulimit 检测失败")?;
    check_selinux(runner)
        .await
        .context("远端节点 SELinux 检测失败")?;
    Ok(())
}

fn install_only_xml(install_path: &str, language: &str) -> String {
    let escaped = install_path
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<?xml version=\"1.0\"?>\n<DATABASE>\n  <LANGUAGE>{language}</LANGUAGE>\n  <INSTALL_PATH>{escaped}</INSTALL_PATH>\n  <INIT_DB>N</INIT_DB>\n</DATABASE>"
    )
}

fn upload_progress_bar(total_bytes: u64) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::with_template(
            "  上传 DMInstall.bin [{bar:40.cyan/blue}] {bytes}/{total_bytes} @ {bytes_per_sec}, ETA {eta}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=>-"),
    );
    pb
}

fn install_spinner() -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.green} {msg} [{elapsed}]")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.set_message("正在静默安装达梦数据库...");
    pb.enable_steady_tick(Duration::from_millis(120));
    pb
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
    async fn test_check_remote_data_path_occupied_returns_true_when_occupied() {
        let runner = MockRunner::new(vec![("[ -d".to_string(), 0, b"occupied\n".to_vec())]);
        let result = check_remote_data_path_occupied(&make_config(), &runner)
            .await
            .unwrap();
        assert!(result, "occupied 输出应返回 true");
    }

    #[tokio::test]
    async fn test_check_remote_dmserver_exists_detects_existing() {
        let runner = MockRunner::new(vec![("test -f".to_string(), 0, b"exists\n".to_vec())]);
        let result = check_remote_dmserver_exists(&make_config(), &runner)
            .await
            .unwrap();
        assert!(result, "exists 输出应返回 true");
    }

    #[tokio::test]
    async fn test_check_remote_dminit_done_detects_existing() {
        let runner = MockRunner::new(vec![("test -f".to_string(), 0, b"exists\n".to_vec())]);
        let result = check_remote_dminit_done(&make_config(), &runner)
            .await
            .unwrap();
        assert!(result, "exists 输出应触发幂等跳过");
    }

    #[tokio::test]
    async fn test_upload_bin_sends_file() {
        let runner = MockRunner::new(vec![]);
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("DMInstall.bin"), b"fake_bin").unwrap();
        let _ = upload_bin(&tmp.path().join("DMInstall.bin"), &runner).await;
        let sftp_log = runner.sftp_log();
        assert!(
            sftp_log.iter().any(|(p, _)| p.contains("DMInstall.bin")),
            "应上传 DMInstall.bin: {:?}",
            sftp_log.iter().map(|(p, _)| p).collect::<Vec<_>>()
        );
    }
}
