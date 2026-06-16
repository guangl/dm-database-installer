use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::time::Duration;

use crate::cli::InstallArgs;
use crate::config::ssh::{SshCredentials, SshTarget};
use crate::config::{CommonConfig, InstallConfig};
use crate::download::fetch_dm_installer_for;
use crate::platform::detect_platform_from_raw;
use crate::ssh::{CommandRunner, SshSession};

use super::{cache_package, checkpoint, generate_passwords, init, service};

const REMOTE_BIN: &str = "/tmp/dm_standalone_DMInstall.bin";
const REMOTE_XML: &str = "/tmp/dm_standalone_install.xml";

use crate::ssh::shell_quote;

pub async fn run(
    args: &InstallArgs,
    common: CommonConfig,
    specific: &InstallConfig,
    target: &SshTarget,
) -> Result<()> {
    crate::ui::print_banner();

    let password = match &target.password {
        Some(p) => p.clone(),
        None => prompt_ssh_password(&target.user, &target.host)?,
    };
    let creds = SshCredentials {
        user: target.user.clone(),
        identity_file: None,
        password: Some(password),
    };

    // [1/6] 环境预检
    crate::ui::step_header("[1/6] 环境预检");
    let session = connect_with_retry(
        &target.host,
        target.ssh_port,
        &creds,
        target.max_retries,
        target.retry_interval_secs,
    )
    .await?;
    // 提前加载 checkpoint（含密码），用于数据库连接检测和续传判断。
    let existing_cp = checkpoint::load(&specific.install_path)?;
    // 通过连接数据库检测服务状态，同时获取版本信息。
    let sysdba_pwd_hint = existing_cp.as_ref().map(|c| c.sysdba_pwd.as_str());
    let db_status = query_remote_db_status(specific, sysdba_pwd_hint, &session).await?;
    let skip_preflight = existing_cp.is_some() || db_status.is_some();
    if skip_preflight {
        crate::ui::log_info("[续] 跳过预检查（从检查点续传）");
    } else {
        check_remote_prerequisites(specific, &session, false).await?;
    }
    if let Some(ver_info) = db_status {
        crate::ui::log_info(&format!(
            "[续] 达梦数据库已运行 ({})",
            super::service::service_name(specific)
        ));
        crate::ui::log_info(&format!("数据库版本: {}", ver_info));
        // 有版本信息时写入缓存，供后续无凭证 re-run 使用
        let cache = format!("{}/.dm_version", specific.install_path);
        let _ = session.sftp_write(&cache, ver_info.as_bytes()).await;
        crate::ui::step_footer();
        return Ok(());
    }
    crate::ui::step_footer();

    // 初始化 checkpoint
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

    // [2/6] 系统准备
    crate::ui::step_header("[2/6] 系统准备");
    if cp.env_setup_done {
        crate::ui::log_info("[续] 系统环境已配置，跳过");
    } else {
        super::env_setup::run(&session).await?;
        cp.env_setup_done = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [3/6] 下载安装包
    crate::ui::step_header("[3/6] 下载安装包");
    let package_path = if cp.installed {
        crate::ui::log_info("[续] dmdbms 已安装，跳过下载");
        None
    } else {
        let path = resolve_package_for_remote(args, &common.installer, &session, &mut cp).await?;
        Some(path)
    };
    crate::ui::step_footer();

    // [4/6] 上传并安装 dmdbms
    crate::ui::step_header("[4/6] 上传并安装");
    if cp.installed {
        crate::ui::log_info(&format!(
            "[续] 跳过安装，dmdbms 已安装至 {}",
            specific.install_path
        ));
    } else {
        let pkg = package_path
            .as_ref()
            .expect("package_path set when !cp.installed");
        if cp.uploaded {
            crate::ui::log_info(&format!(
                "[续] 跳过上传，DMInstall.bin 已在远端 {}",
                REMOTE_BIN
            ));
            session
                .sftp_set_permissions(REMOTE_BIN, 0o755)
                .await
                .context("重设 DMInstall.bin 执行权限失败，请手动删除远端 /tmp/dm_standalone_DMInstall.bin 后重试")?;
        } else {
            upload_and_extract_on_remote(pkg, &session).await?;
            cp.uploaded = true;
            cp.save()?;
        }
        if check_remote_dmserver_exists(specific, &session).await? {
            anyhow::bail!(
                "安装目录 {} 已存在达梦数据库（dmserver），\n\
                 请先卸载或在配置文件中修改 install_path",
                specific.install_path
            );
        }
        run_remote_install(specific, &session).await?;
        cp.installed = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [5/6] 初始化数据库
    crate::ui::step_header("[5/6] 初始化数据库");
    if cp.db_inited {
        crate::ui::log_info("[续] 跳过 dminit，数据库实例已初始化");
    } else if check_remote_dminit_done(specific, &session).await? {
        crate::ui::log_info("[续] 跳过 dminit，数据库实例已初始化");
        cp.db_inited = true;
        cp.save()?;
    } else {
        if !cp.installed && check_remote_data_path_occupied(specific, &session).await? {
            anyhow::bail!(
                "数据目录 {} 已有文件，可能存在旧实例，\n\
                 请先清理或在配置文件中修改 data_path",
                specific.data_path
            );
        }
        init::run_dminit(&session, specific, &sysdba_pwd, &sysauditor_pwd).await?;
        init::write_dmarch_ini(&session, specific).await?;
        cp.db_inited = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [6/6] 注册服务
    crate::ui::step_header("[6/6] 注册服务");
    if cp.services_done {
        crate::ui::log_info("[续] 服务已注册，跳过");
    } else {
        service::register_and_start(&session, specific).await?;
        if let Some(ver) = query_db_version_via_disql(specific, &sysdba_pwd, &session).await {
            let cache = format!("{}/.dm_version", specific.install_path);
            let _ = session.sftp_write(&cache, ver.as_bytes()).await;
        }
        cp.services_done = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    crate::ui::print_success(specific, &sysdba_pwd, &sysauditor_pwd);
    if let Some(cached) = &cp.package_cache {
        let _ = std::fs::remove_file(cached);
    }
    checkpoint::Checkpoint::remove()?;
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

    if let Some(p) = &args.package {
        crate::ui::log_info(&format!("使用本地安装包 (CLI --package): {}", p.display()));
        return Ok(p.clone());
    }
    if let Some(url) = &args.url {
        crate::ui::log_info(&format!("下载安装包 (CLI --url): {}", url));
        let handle = crate::download::fetch_from_url(url, None).await?;
        let cached = cache_package(&handle.path)?;
        cp.package_cache = Some(cached.to_string_lossy().into_owned());
        cp.save()?;
        return Ok(cached);
    }

    match installer {
        InstallerSource::LocalFile(path) => {
            crate::ui::log_info(&format!("使用本地安装包 (config.toml): {}", path.display()));
            Ok(path.clone())
        }
        InstallerSource::Url(url) => {
            if let Some(cached) = cp
                .package_cache
                .as_ref()
                .map(std::path::Path::new)
                .filter(|p| p.exists())
            {
                crate::ui::log_info(&format!(
                    "[续] 跳过下载，使用已缓存安装包: {}",
                    cached.display()
                ));
                return Ok(cached.to_path_buf());
            }
            let handle = crate::download::fetch_from_url(url, None).await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
        InstallerSource::Auto => {
            if let Some(cached) = cp
                .package_cache
                .as_ref()
                .map(std::path::Path::new)
                .filter(|p| p.exists())
            {
                crate::ui::log_info(&format!(
                    "[续] 跳过下载，使用已缓存安装包: {}",
                    cached.display()
                ));
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
    if lang.trim().to_lowercase().contains("zh") {
        "ZH"
    } else {
        "EN"
    }
}

async fn detect_remote_platform(runner: &dyn CommandRunner) -> crate::platform::Platform {
    let uname = exec_remote_str(runner, "uname -m").await;
    let cpuinfo = exec_remote_str(runner, "cat /proc/cpuinfo 2>/dev/null || true").await;
    let os_release = exec_remote_str(runner, "cat /etc/os-release 2>/dev/null || true").await;
    detect_platform_from_raw(&uname, &cpuinfo, &os_release)
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

/// 检测远端 dminit 是否已完成：{data_path}/DAMENG/dm.ini 存在则认为已完成。
/// dminit 以 DB_NAME=DAMENG 初始化，实例数据在 {data_path}/DAMENG/ 子目录中。
/// 检测 {data_path}/DAMENG/dm.ini 是否存在，用于 dminit 幂等保护。
async fn check_remote_dminit_done(
    config: &InstallConfig,
    runner: &dyn CommandRunner,
) -> Result<bool> {
    let dm_ini = super::service::dm_ini_path(config);
    let cmd = format!(
        "test -f {} && echo exists || echo absent",
        shell_quote(&dm_ini)
    );
    let (output, _) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("远端幂等检测失败: {e}"))?;
    Ok(String::from_utf8_lossy(&output).trim() == "exists")
}

/// 检测 dmserver 是否在运行并连接数据库获取版本信息。
/// 返回 Ok(Some(version)) 表示数据库可访问，Ok(None) 表示未运行。
///
/// 步骤：
///   1. bash TCP 探测端口（快速预检，不依赖任何工具）
///   2. 若端口开放 + 有密码 → 写临时脚本文件（绕过密码中 @ 的引号问题）→ 用 disql 连接并查 V$VERSION
///   3. 若无密码或连接失败 → 返回端口可达的标记信息
async fn query_remote_db_status(
    config: &InstallConfig,
    sysdba_pwd: Option<&str>,
    runner: &dyn CommandRunner,
) -> Result<Option<String>> {
    // [1] TCP 端口快速探测
    let tcp_cmd = format!(
        "bash -c 'echo >/dev/tcp/127.0.0.1/{port}' 2>/dev/null && echo open || echo closed",
        port = config.port,
    );
    let (tcp_out, _) = runner.exec(&tcp_cmd).await.unwrap_or_default();
    if String::from_utf8_lossy(&tcp_out).trim() != "open" {
        return Ok(None);
    }

    // [2] 端口可达 → 尝试通过 disql 连接数据库获取版本
    if let Some(pwd) = sysdba_pwd
        && let Some(ver) = query_db_version_via_disql(config, pwd, runner).await
    {
        return Ok(Some(ver));
    }

    // [3] 无密码（或连接失败）：先查缓存文件，再尝试 disql banner（不需要连接）
    let ver = query_version_from_cache_or_banner(config, runner).await;
    Ok(Some(
        ver.unwrap_or_else(|| format!("端口 {} 已监听", config.port)),
    ))
}

/// 通过 disql 连接达梦数据库并查询 V$VERSION 获取版本字符串。
///
/// 密码通过 stdin 而非连接串传入，避免密码中 @ 被当作连接串分隔符误解析。
/// 临时脚本写入文件（含实际换行符），连接形式：`disql SYSDBA@host:port < input_file`
/// 其中 input_file 第一行是密码（DM disql 无密码段时从 stdin 读取），后续是 SQL。
async fn query_db_version_via_disql(
    config: &InstallConfig,
    sysdba_pwd: &str,
    runner: &dyn CommandRunner,
) -> Option<String> {
    let disql = format!("{}/bin/disql", config.install_path);
    // input 文件：第 1 行 = 密码，第 2 行 = SQL，第 3 行 = exit
    let input_content = format!("{}\nSELECT ID_CODE FROM V$VERSION;\nexit;\n", sysdba_pwd);
    // 生成脚本：用 heredoc 写 input 文件，再把 disql 的 stdin 重定向到该文件
    // 单引号 heredoc (<<'EOF') 使 shell 不对 $ 展开，但密码中的单引号需转义
    let escaped_input = input_content.replace('\'', "'\\''");
    let script = format!(
        "#!/bin/sh\ncat > /tmp/dm_ver_in.txt << 'DM_VER_EOF'\n{escaped}\nDM_VER_EOF\n\
         {disql} SYSDBA@localhost:{port} < /tmp/dm_ver_in.txt 2>&1\n\
         ret=$?; rm -f /tmp/dm_ver_in.txt; exit $ret\n",
        escaped = escaped_input,
        disql = shell_quote(&disql),
        port = config.port,
    );
    const VER_SCRIPT: &str = "/tmp/dm_ver_check.sh";
    if runner
        .sftp_write(VER_SCRIPT, script.as_bytes())
        .await
        .is_err()
    {
        return None;
    }
    let _ = runner.exec(&format!("chmod +x {}", VER_SCRIPT)).await;
    let result = runner
        .exec(&format!("su - dmdba -c {} 2>&1", shell_quote(VER_SCRIPT)))
        .await;
    let _ = runner.exec(&format!("rm -f {}", VER_SCRIPT)).await;

    match result {
        Ok((out, _)) => parse_dm_version(&String::from_utf8_lossy(&out)),
        Err(_) => None,
    }
}

/// 无密码时的版本获取策略：先读安装目录下的版本缓存文件，再尝试 disql banner。
/// 缓存文件由首次安装成功后写入（此时有密码可用），供后续无密码 re-run 读取。
async fn query_version_from_cache_or_banner(
    config: &InstallConfig,
    runner: &dyn CommandRunner,
) -> Option<String> {
    // 优先读缓存文件（首次安装时写入）
    let cache_path = format!("{}/.dm_version", config.install_path);
    if let Ok((data, _)) = runner
        .exec(&format!("cat {} 2>/dev/null", shell_quote(&cache_path)))
        .await
    {
        let cached = String::from_utf8_lossy(&data).trim().to_string();
        if !cached.is_empty() {
            return Some(cached);
        }
    }
    // 降级：disql 启动时若打印 banner 则解析；非交互式下通常不输出，作为最后尝试
    let disql = format!("{}/bin/disql", config.install_path);
    let cmd = format!(
        "echo 'exit;' | su - dmdba -c {} 2>&1 | head -3",
        shell_quote(&disql),
    );
    runner
        .exec(&cmd)
        .await
        .ok()
        .and_then(|(out, _)| parse_dm_version(&String::from_utf8_lossy(&out)))
}

/// 从 disql 输出中解析达梦数据库版本字符串。
///
/// V$VERSION.ID_CODE 的查询结果行格式：
///   `1          --03134284552-20260414-322369-20221`
/// 取第一个数据行（跳过表头和分隔线）的第二列即为版本字符串。
fn parse_dm_version(output: &str) -> Option<String> {
    // 找到 ID_CODE 表头后的第一个数据行：跳过 "行号" 标题行和 "---" 分隔行
    let mut past_header = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.contains("ID_CODE") {
            past_header = true;
            continue;
        }
        if past_header && trimmed.starts_with("---") {
            continue;
        }
        if past_header && !trimmed.is_empty() {
            // 行格式："1          --03134284552-..." → 取第二列
            let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
            if parts.len() == 2 {
                let id_code = parts[1].trim();
                if !id_code.is_empty() {
                    return Some(id_code.to_string());
                }
            }
        }
    }
    None
}

/// 根据包类型上传 DMInstall.bin 到远端。
/// - 若本地包本身就是 DMInstall.bin → 直接上传
/// - 否则（ISO/其他）→ 本地提取 DMInstall.bin 后再上传，不依赖远端 mount
async fn upload_and_extract_on_remote(
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
    // ISO：本地提取 DMInstall.bin，再上传二进制，彻底不依赖远端 loop mount
    crate::ui::log_info("本地提取 DMInstall.bin...");
    let extract_dir = crate::install::package::extract_dminstall_bin(package_path)
        .context("本地提取 DMInstall.bin 失败")?;
    let bin_path = extract_dir.path().join("DMInstall.bin");
    upload_bin(&bin_path, runner).await
    // extract_dir 在此 drop，自动清理临时目录
}

/// 上传 DMInstall.bin 到远端 /tmp。
async fn upload_bin(bin_path: &Path, runner: &dyn CommandRunner) -> Result<()> {
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
async fn run_remote_install(config: &InstallConfig, runner: &dyn CommandRunner) -> Result<()> {
    let language = detect_remote_language(runner).await;
    let xml = install_only_xml(&config.install_path, language);
    runner
        .sftp_write(REMOTE_XML, xml.as_bytes())
        .await
        .context("SFTP 上传安装 XML 失败")?;

    let spinner = install_spinner();
    // 输出重定向到日志；失败时用 sed 过滤 ANSI 转义码后输出实际错误文本
    const REMOTE_LOG: &str = "/tmp/dm_standalone_install.log";
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

async fn check_remote_prerequisites(
    specific: &InstallConfig,
    runner: &dyn CommandRunner,
    skip_port_check: bool,
) -> anyhow::Result<()> {
    use crate::install::preflight::{
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
        check_port_available(runner, specific.ap_port)
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
    use crate::ssh::MockRunner;

    fn make_config() -> InstallConfig {
        InstallConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            instance_name: "DMSERVER".to_string(),
            port: 5236,
            ap_port: 4236,
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
        let result = check_remote_data_path_occupied(&make_config(), &runner)
            .await
            .unwrap();
        assert!(result, "occupied 输出应返回 true");
    }

    #[tokio::test]
    async fn test_check_remote_data_path_occupied_returns_false_when_clean() {
        let runner = MockRunner::new(vec![("[ -d".to_string(), 0, b"clean\n".to_vec())]);
        let result = check_remote_data_path_occupied(&make_config(), &runner)
            .await
            .unwrap();
        assert!(!result, "clean 输出应返回 false");
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
    async fn test_check_remote_dmserver_exists_returns_false_when_absent() {
        let runner = MockRunner::new(vec![("test -f".to_string(), 0, b"absent\n".to_vec())]);
        let result = check_remote_dmserver_exists(&make_config(), &runner)
            .await
            .unwrap();
        assert!(!result, "absent 输出应返回 false");
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
    async fn test_check_remote_dminit_done_proceeds_when_absent() {
        let runner = MockRunner::new(vec![("test -f".to_string(), 0, b"absent\n".to_vec())]);
        let result = check_remote_dminit_done(&make_config(), &runner)
            .await
            .unwrap();
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
    async fn test_run_dminit_contains_passwords() {
        let runner = MockRunner::new(vec![]);
        init::run_dminit(&runner, &make_config(), "Sysdba1@Pass", "Audit2@Pass")
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
    async fn test_run_dminit_quotes_paths() {
        let runner = MockRunner::new(vec![]);
        init::run_dminit(&runner, &make_config(), "Pwd1@Test1", "Pwd2@Test2")
            .await
            .unwrap();
        let exec_log = runner.exec_log();
        assert!(
            exec_log
                .iter()
                .any(|cmd| cmd.contains("/opt/dmdbms/bin/dminit")),
            "dminit 路径应出现在命令中: {:?}",
            exec_log
        );
        assert!(
            exec_log.iter().any(|cmd| cmd.starts_with("su - dmdba -c")),
            "dminit 应以 dmdba 用户身份执行: {:?}",
            exec_log
        );
    }
}
