use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::time::Duration;

use crate::cli::InstallArgs;
use crate::common::download::fetch_dm_installer_for;
use crate::common::ssh::{CommandRunner, SshSession};
use crate::common::sysinfo::detect_platform_from_raw;
use crate::config::ssh::{SshCredentials, SshTarget};
use crate::config::{CommonConfig, InstallConfig};

use super::{cache_package, checkpoint, generate_passwords, print_generated_credentials, verify_checksum};

const REMOTE_BIN: &str = "/tmp/dm_standalone_DMInstall.bin";
const REMOTE_XML: &str = "/tmp/dm_standalone_install.xml";

use crate::common::shell_quote;

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
        port: target.ssh_port,
    };
    let session = connect_with_retry(
        &target.host,
        target.ssh_port,
        &creds,
        target.max_retries,
        target.retry_interval_secs,
    )
    .await?;

    // [1/5] 幂等检测：dm.ini 已存在说明安装已全部完成
    if check_remote_dminit_done(specific, &session).await? {
        return Ok(());
    }

    // 加载或创建 checkpoint（跨重试持久化密码和各步骤进度）
    let existing_cp = checkpoint::load(&specific.install_path)?;
    let (sysdba_pwd, sysauditor_pwd) = match &existing_cp {
        Some(c) => (c.sysdba_pwd.clone(), c.sysauditor_pwd.clone()),
        None => generate_passwords(),
    };
    let mut cp = existing_cp.unwrap_or_else(|| {
        checkpoint::Checkpoint::new(
            &specific.install_path,
            sysdba_pwd.clone(),
            sysauditor_pwd.clone(),
        )
    });
    cp.save()?;

    // [2/5] 获取本地安装包（自动下载时缓存到 CWD，支持续传）
    let package_path = resolve_package_for_remote(args, &common.installer, &session, &mut cp).await?;
    verify_checksum(args, &package_path)?;

    // [3/5] 上传 DMInstall.bin 到远端
    if cp.uploaded {
        println!("[续] 跳过上传，DMInstall.bin 已在远端 {}", REMOTE_BIN);
    } else {
        let extract_dir = crate::standalone::package::extract_dminstall_bin(&package_path)
            .context("提取 DMInstall.bin 失败")?;
        upload_bin(&extract_dir.path().join("DMInstall.bin"), &session).await?;
        cp.uploaded = true;
        cp.save()?;
    }

    // [4/5] 远端静默安装 dmdbms
    if cp.installed {
        println!("[续] 跳过安装，dmdbms 已安装至 {}", specific.install_path);
    } else if check_remote_dmserver_exists(specific, &session).await? {
        anyhow::bail!(
            "安装目录 {} 已存在达梦数据库（dmserver），\n\
             请先卸载或在配置文件中修改 install_path",
            specific.install_path
        );
    } else {
        run_remote_install(specific, &session).await?;
        cp.installed = true;
        cp.save()?;
    }

    // [5/6] 远端 dminit 初始化
    // 非续传场景下预先检测 data_path 是否已有内容（旧实例或手动数据），避免 dminit 冲突后才报错
    if !cp.installed && check_remote_data_path_occupied(specific, &session).await? {
        anyhow::bail!(
            "数据目录 {} 已有文件，可能存在旧实例，\n\
             请先清理或在配置文件中修改 data_path",
            specific.data_path
        );
    }
    run_dminit_remote(specific, &sysdba_pwd, &sysauditor_pwd, &session).await?;
    run_write_dmarch_ini_remote(specific, &session).await?;

    // [6/6] 远端注册并启动 DM 服务
    run_service_remote(specific, &session).await?;

    print_generated_credentials(&sysdba_pwd, &sysauditor_pwd);
    if let Some(cached) = &cp.package_cache {
        let _ = std::fs::remove_file(cached);
    }
    checkpoint::Checkpoint::remove()?;
    tracing::info!("单机 SSH 远程安装完成");
    Ok(())
}

/// 获取安装包路径：CLI > config > checkpoint 缓存 > 自动下载（按远端平台）。
async fn resolve_package_for_remote(
    args: &InstallArgs,
    installer: &crate::config::InstallerSource,
    runner: &dyn CommandRunner,
    cp: &mut checkpoint::Checkpoint,
) -> Result<std::path::PathBuf> {
    use crate::config::InstallerSource;
    tracing::info!("[2/6] 获取安装包");

    if let Some(p) = &args.package {
        println!("使用本地安装包 (CLI --package): {}", p.display());
        return Ok(p.clone());
    }
    if let Some(url) = &args.url {
        println!("下载安装包 (CLI --url): {}", url);
        let handle = crate::common::download::fetch_from_url(url).await?;
        let cached = cache_package(&handle.path)?;
        cp.package_cache = Some(cached.to_string_lossy().into_owned());
        cp.save()?;
        return Ok(cached);
    }

    match installer {
        InstallerSource::LocalFile(path) => {
            println!("使用本地安装包 (config.toml): {}", path.display());
            Ok(path.clone())
        }
        InstallerSource::Url(url) => {
            if let Some(cached) = cp.package_cache.as_ref().map(std::path::Path::new).filter(|p| p.exists()) {
                println!("[续] 跳过下载，使用已缓存安装包: {}", cached.display());
                return Ok(cached.to_path_buf());
            }
            let handle = crate::common::download::fetch_from_url(url).await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
        InstallerSource::Auto => {
            if let Some(cached) = cp.package_cache.as_ref().map(std::path::Path::new).filter(|p| p.exists()) {
                println!("[续] 跳过下载，使用已缓存安装包: {}", cached.display());
                return Ok(cached.to_path_buf());
            }
            let platform = detect_remote_platform(runner).await;
            let handle = fetch_dm_installer_for(&platform).await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
    }
}

async fn detect_remote_language(runner: &dyn CommandRunner) -> &'static str {
    let lang = exec_remote_str(runner, "echo $LANG").await;
    let language = if lang.trim().to_lowercase().contains("zh") { "ZH" } else { "EN" };
    tracing::debug!("远端 $LANG={:?} -> 安装语言: {}", lang.trim(), language);
    language
}

async fn detect_remote_platform(runner: &dyn CommandRunner) -> crate::common::sysinfo::Platform {
    let uname = exec_remote_str(runner, "uname -m").await;
    let cpuinfo = exec_remote_str(runner, "cat /proc/cpuinfo 2>/dev/null || true").await;
    let os_release = exec_remote_str(runner, "cat /etc/os-release 2>/dev/null || true").await;
    let platform = detect_platform_from_raw(&uname, &cpuinfo, &os_release);
    tracing::info!(
        "远端平台: arch={}, cpu={:?}, os={:?}",
        platform.arch,
        platform.cpu,
        platform.os
    );
    platform
}

async fn exec_remote_str(runner: &dyn CommandRunner, cmd: &str) -> String {
    runner
        .exec(cmd)
        .await
        .map(|(bytes, _)| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// 检测远端 data_path 目录是否非空（说明有旧数据，dminit 会冲突）。
async fn check_remote_data_path_occupied(
    config: &InstallConfig,
    runner: &dyn CommandRunner,
) -> Result<bool> {
    let cmd = format!(
        "[ -d {path} ] && [ -n \"$(ls -A {path} 2>/dev/null)\" ] && echo occupied || echo clean",
        path = shell_quote(&config.data_path)
    );
    let (output, _) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("远端数据目录检测失败: {e}"))?;
    Ok(String::from_utf8_lossy(&output).trim() == "occupied")
}

/// 检测远端 install_path/bin/dmserver 是否已存在（说明有旧安装）。
async fn check_remote_dmserver_exists(
    config: &InstallConfig,
    runner: &dyn CommandRunner,
) -> Result<bool> {
    let dmserver = format!("{}/bin/dmserver", config.install_path);
    let cmd = format!(
        "test -f {} && echo exists || echo absent",
        shell_quote(&dmserver)
    );
    let (output, _) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("远端 dmserver 检测失败: {e}"))?;
    Ok(String::from_utf8_lossy(&output).trim() == "exists")
}

/// 检测远端 dminit 是否已完成：data_path/dm.ini 存在则认为已完成。
async fn check_remote_dminit_done(
    config: &InstallConfig,
    runner: &dyn CommandRunner,
) -> Result<bool> {
    tracing::info!("[1/6] 远端幂等性检测");
    let dm_ini = format!("{}/dm.ini", config.data_path);
    let cmd = format!(
        "test -f {} && echo exists || echo absent",
        shell_quote(&dm_ini)
    );
    let (output, _) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("远端幂等检测失败: {e}"))?;
    let result = String::from_utf8_lossy(&output);
    if result.trim() == "exists" {
        println!("已检测到远端达梦实例 ({})，跳过安装", dm_ini);
        return Ok(true);
    }
    Ok(false)
}

/// 上传 DMInstall.bin 到远端 /tmp。
async fn upload_bin(bin_path: &Path, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[3/6] 上传 DMInstall.bin");
    let bin_bytes = std::fs::read(bin_path)
        .with_context(|| format!("读取 DMInstall.bin 失败: {}", bin_path.display()))?;
    let pb = upload_progress_bar(bin_bytes.len() as u64);
    runner
        .sftp_write_with_progress(REMOTE_BIN, &bin_bytes, &|n| pb.inc(n))
        .await
        .context("SFTP 上传 DMInstall.bin 失败")?;
    pb.finish_with_message("上传完成");
    Ok(())
}

/// 上传安装 XML 并执行静默安装；成功后远端自动清理临时文件。
async fn run_remote_install(config: &InstallConfig, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[4/6] 远端静默安装 dmdbms");
    let language = detect_remote_language(runner).await;
    let xml = install_only_xml(&config.install_path, language);
    runner
        .sftp_write(REMOTE_XML, xml.as_bytes())
        .await
        .context("SFTP 上传安装 XML 失败")?;

    let spinner = install_spinner();
    // 成功时清理临时文件；失败时保留 bin，方便下次续传跳过上传步骤
    let cmd = format!(
        "chmod +x {bin} && {bin} -q {xml}; ret=$?; [ $ret -eq 0 ] && rm -f {xml} {bin}; exit $ret",
        bin = shell_quote(REMOTE_BIN),
        xml = shell_quote(REMOTE_XML),
    );
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("远端安装 dmdbms 失败: {e}"))?;
    spinner.finish_with_message("安装完成");
    tracing::info!("远端 dmdbms 安装成功: {}", config.install_path);
    Ok(())
}

async fn run_dminit_remote(
    config: &InstallConfig,
    sysdba_pwd: &str,
    sysauditor_pwd: &str,
    runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("[5/6] 远端 dminit 初始化");
    let dminit = format!("{}/bin/dminit", config.install_path);
    let cmd = format!(
        "{} PATH={} DB_NAME=DAMENG INSTANCE_NAME={} PORT_NUM={} PAGE_SIZE={} EXTENT_SIZE={} CHARSET={} CASE_SENSITIVE={} ARCH_INI=1 SYSDBA_PWD={} SYSAUDITOR_PWD={}",
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

async fn run_write_dmarch_ini_remote(config: &InstallConfig, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[5b/6] 远端写入 dmarch.ini");
    let arch_path = config.archive.arch_path.clone()
        .unwrap_or_else(|| format!("{}/arch", config.data_path));
    runner
        .exec(&format!("mkdir -p {}", shell_quote(&arch_path)))
        .await
        .map_err(|e| anyhow::anyhow!("远端创建归档目录失败: {e}"))?;
    let content = crate::standalone::init::generate_standalone_dmarch_ini(config);
    let dmarch_path = format!("{}/dmarch.ini", config.data_path);
    runner
        .sftp_write(&dmarch_path, content.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("远端写入 dmarch.ini 失败: {e}"))?;
    tracing::info!("远端 dmarch.ini 写入完成: {}", dmarch_path);
    Ok(())
}

async fn run_service_remote(config: &InstallConfig, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[6/6] 远端注册并启动 DM 服务（DMAP + dmserver）");

    // 服务注册需要 root 权限，提前检测
    let (uid_out, _) = runner.exec("id -u").await.unwrap_or_default();
    let remote_uid = String::from_utf8_lossy(&uid_out).trim().to_string();
    anyhow::ensure!(
        remote_uid == "0",
        "远端服务注册需要 root 权限（远端 UID: {}）。\n\
         请在 standalone.toml 的 ssh_target 中配置 root 用户",
        remote_uid
    );

    let script = format!("{}/script/root/dm_service_installer.sh", config.install_path);
    let quoted_script = shell_quote(&script);
    let dm_ini = super::service::dm_ini_path(config);
    let db_svc = super::service::service_name(config);
    let dmap_svc = super::service::DMAP_SERVICE_NAME;

    // 注册并启动 DMAP（辅助进程，先于 dmserver）
    remote_register_and_start(
        runner,
        &quoted_script,
        dmap_svc,
        &["-t", "dmap"],
    ).await.map_err(|e| anyhow::anyhow!("远端 DMAP 服务注册/启动失败: {e}"))?;

    // 注册并启动 dmserver
    remote_register_and_start(
        runner,
        &quoted_script,
        &db_svc,
        &["-t", "dmserver", "-p", &dm_ini, "-m", "auto"],
    ).await.map_err(|e| anyhow::anyhow!("远端数据库服务注册/启动失败: {e}"))?;
    Ok(())
}

/// 通用：检测远端服务状态，按需注册，然后启动并 enable。
async fn remote_register_and_start(
    runner: &dyn CommandRunner,
    quoted_script: &str,
    svc_name: &str,
    installer_args: &[&str],
) -> Result<()> {
    // 已运行则跳过
    let (active_out, _) = runner
        .exec(&format!(
            "systemctl is-active {} 2>/dev/null && echo active || echo inactive",
            shell_quote(svc_name)
        ))
        .await
        .unwrap_or_default();
    if String::from_utf8_lossy(&active_out).trim() == "active" {
        println!("[续] 远端服务 {} 已在运行，跳过注册", svc_name);
        return Ok(());
    }

    // 未注册则注册
    let check_cmd = format!(
        "test -f /etc/systemd/system/{s}.service || test -f /etc/init.d/{s} \
         && echo registered || echo unregistered",
        s = svc_name
    );
    let (check_out, _) = runner.exec(&check_cmd).await.unwrap_or_default();
    if String::from_utf8_lossy(&check_out).trim() != "registered" {
        let mut cmd = format!("chmod +x {script} && bash {script}", script = quoted_script);
        for arg in installer_args {
            cmd.push(' ');
            cmd.push_str(&shell_quote(arg));
        }
        runner
            .exec(&cmd)
            .await
            .map_err(|e| anyhow::anyhow!("执行服务注册脚本失败: {e}"))?;
    } else {
        println!("[续] 远端服务 {} 已注册，跳过注册步骤", svc_name);
    }

    // 启动并 enable（enable 失败仅告警，容器环境可能不支持）
    let start_cmd = format!(
        "systemctl start {s} && systemctl enable {s} 2>/dev/null \
         || (service {s} start 2>/dev/null; true)",
        s = shell_quote(svc_name)
    );
    runner
        .exec(&start_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("启动服务 {} 失败: {e}", svc_name))?;

    println!("远端服务 {} 已启动并设置为开机自启", svc_name);
    Ok(())
}

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
            archive: Default::default(),
            ssh_target: None,
        }
    }

    #[tokio::test]
    async fn test_check_remote_data_path_occupied_returns_true_when_occupied() {
        let runner = MockRunner::new(vec![("[ -d".to_string(), 0, b"occupied\n".to_vec())]);
        let result = check_remote_data_path_occupied(&make_config(), &runner).await.unwrap();
        assert!(result, "occupied 输出应返回 true");
    }

    #[tokio::test]
    async fn test_check_remote_data_path_occupied_returns_false_when_clean() {
        let runner = MockRunner::new(vec![("[ -d".to_string(), 0, b"clean\n".to_vec())]);
        let result = check_remote_data_path_occupied(&make_config(), &runner).await.unwrap();
        assert!(!result, "clean 输出应返回 false");
    }

    #[tokio::test]
    async fn test_check_remote_dmserver_exists_detects_existing() {
        let runner = MockRunner::new(vec![("test -f".to_string(), 0, b"exists\n".to_vec())]);
        let result = check_remote_dmserver_exists(&make_config(), &runner).await.unwrap();
        assert!(result, "exists 输出应返回 true");
    }

    #[tokio::test]
    async fn test_check_remote_dmserver_exists_returns_false_when_absent() {
        let runner = MockRunner::new(vec![("test -f".to_string(), 0, b"absent\n".to_vec())]);
        let result = check_remote_dmserver_exists(&make_config(), &runner).await.unwrap();
        assert!(!result, "absent 输出应返回 false");
    }

    #[tokio::test]
    async fn test_check_remote_dminit_done_detects_existing() {
        let runner = MockRunner::new(vec![("test -f".to_string(), 0, b"exists\n".to_vec())]);
        let result = check_remote_dminit_done(&make_config(), &runner).await.unwrap();
        assert!(result, "exists 输出应触发幂等跳过");
    }

    #[tokio::test]
    async fn test_check_remote_dminit_done_proceeds_when_absent() {
        let runner = MockRunner::new(vec![("test -f".to_string(), 0, b"absent\n".to_vec())]);
        let result = check_remote_dminit_done(&make_config(), &runner).await.unwrap();
        assert!(!result, "absent 输出应允许继续安装");
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

    #[tokio::test]
    async fn test_run_dminit_remote_contains_passwords() {
        let runner = MockRunner::new(vec![]);
        run_dminit_remote(&make_config(), "Sysdba1@Pass", "Audit2@Pass", &runner)
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
        run_dminit_remote(&make_config(), "Pwd1@Test1", "Pwd2@Test2", &runner)
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
