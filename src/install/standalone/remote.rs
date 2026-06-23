use anyhow::{Context, Result};

use crate::cli::InstallArgs;
use crate::config::ssh::{SshCredentials, SshTarget};
use crate::config::{CommonConfig, InstallConfig};
use crate::download::fetch_dm_installer_for;
use crate::install::remote_common::{
    check_remote_data_path_occupied, check_remote_dminit_done, check_remote_dmserver_exists,
    check_remote_prerequisites, connect_with_retry, detect_remote_platform, prompt_ssh_password,
    run_remote_install, upload_and_extract_on_remote,
};
use crate::ssh::CommandRunner;

use super::{cache_package, checkpoint, generate_passwords, init, service};

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

    // [1/10] 环境预检
    crate::ui::step_header("[1/10] 环境预检");
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

    // [2/10] 系统准备
    crate::ui::step_header("[2/10] 系统准备");
    if cp.env_setup_done {
        crate::ui::log_info("[续] 系统环境已配置，跳过");
    } else {
        super::env_setup::run(&session).await?;
        cp.env_setup_done = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [3/10] 下载安装包
    crate::ui::step_header("[3/10] 下载安装包");
    let package_path = if cp.installed {
        crate::ui::log_info("[续] dmdbms 已安装，跳过下载");
        None
    } else {
        let path = resolve_package_for_remote(args, &common.installer, &session, &mut cp).await?;
        Some(path)
    };
    crate::ui::step_footer();

    // [4/10] 上传并安装 dmdbms
    crate::ui::step_header("[4/10] 上传并安装");
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
                crate::install::remote_common::REMOTE_BIN
            ));
            session
                .sftp_set_permissions(crate::install::remote_common::REMOTE_BIN, 0o755)
                .await
                .context("重设 DMInstall.bin 执行权限失败，请手动删除远端 /tmp/dm_remote_DMInstall.bin 后重试")?;
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

    // [5/10] 初始化数据库
    crate::ui::step_header("[5/10] 初始化数据库");
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
        cp.db_inited = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [6/10] 注册服务
    crate::ui::step_header("[6/10] 注册服务");
    let dm_version = if cp.services_done {
        crate::ui::log_info("[续] 服务已注册，跳过");
        query_version_from_cache_or_banner(specific, &session).await
    } else {
        service::register_and_start(&session, specific).await?;
        let ver = query_db_version_via_disql(specific, &sysdba_pwd, &session).await;
        if let Some(ref v) = ver {
            let cache = format!("{}/.dm_version", specific.install_path);
            let _ = session.sftp_write(&cache, v.as_bytes()).await;
        }
        cp.services_done = true;
        cp.save()?;
        ver
    };
    crate::ui::step_footer();

    // [7/10] 配置归档（在线开启，dmserver 无需重启）
    crate::ui::step_header("[7/10] 配置归档");
    if cp.arch_configured {
        crate::ui::log_info("[续] 归档已配置，跳过");
    } else {
        super::archive::enable_archive_online(&session, specific, &sysdba_pwd).await?;
        cp.arch_configured = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [8/10] 配置备份作业
    crate::ui::step_header("[8/10] 配置备份作业");
    if cp.backup_configured {
        crate::ui::log_info("[续] 备份作业已配置，跳过");
    } else {
        super::backup::configure_jobs(&session, specific, &sysdba_pwd).await?;
        cp.backup_configured = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [9/10] 开启 SQL 日志
    crate::ui::step_header("[9/10] 开启 SQL 日志");
    if cp.sql_log_enabled {
        crate::ui::log_info("[续] SQL 日志已开启，跳过");
    } else {
        super::sql_log::enable(&session, specific, &sysdba_pwd).await?;
        cp.sql_log_enabled = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [10/10] 应用参数调优（执行 AutoParaAdj 脚本并重启 dmserver 使其生效）
    crate::ui::step_header("[10/10] 应用参数调优");
    if cp.param_tuned {
        crate::ui::log_info("[续] 参数调优已应用，跳过");
    } else {
        super::param_tune::apply_and_restart(&session, specific, &sysdba_pwd).await?;
        cp.param_tuned = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    crate::ui::print_success(
        specific,
        &sysdba_pwd,
        &sysauditor_pwd,
        dm_version.as_deref(),
    );
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
            let handle = crate::download::fetch_from_url(url, None).await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
        InstallerSource::Auto => {
            let platform = detect_remote_platform(runner).await;
            let handle = fetch_dm_installer_for(&platform).await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
    }
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
            page_size: 32,
            charset: 1,
            case_sensitive: true,
            extent_size: 32,
            archive: Default::default(),
            backup: Default::default(),
            ssh_target: None,
        }
    }

    #[tokio::test]
    async fn test_resolve_package_for_remote_skips_download_when_cached() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cached_pkg = tmp.path().join(".dm_cache_dm8.iso");
        std::fs::write(&cached_pkg, b"fake iso").unwrap();

        let args = InstallArgs {
            package: None,
            url: None,
        };
        let mut cp = checkpoint::Checkpoint::new("/opt/dmdbms", "pwd1".into(), "pwd2".into());
        cp.package_cache = Some(cached_pkg.to_string_lossy().into_owned());
        let runner = MockRunner::new(vec![]);

        let resolved = resolve_package_for_remote(
            &args,
            &crate::config::InstallerSource::Auto,
            &runner,
            &mut cp,
        )
        .await
        .unwrap();
        assert_eq!(resolved, cached_pkg);
        assert!(
            runner.exec_log().is_empty(),
            "缓存命中时不应执行任何远端命令（即不应触发平台探测/下载）: {:?}",
            runner.exec_log()
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
