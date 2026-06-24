use anyhow::{Context, Result};
use std::path::Path;

use crate::cli::InstallArgs;
use crate::config::{CommonConfig, InstallConfig};
use crate::install::steps::{
    archive, backup, env_setup, init, package, param_tune, preflight as cpf, service,
    silent_install, sql_log,
};
use crate::ssh::LocalRunner;

pub mod checkpoint;
pub mod remote;

/// 单机安装入口。common 和 specific 已由调用方从配置文件加载并验证。
pub async fn run(args: &InstallArgs, common: CommonConfig, specific: InstallConfig) -> Result<()> {
    if let Some(target) = &specific.ssh_target
        && !is_local_host(&target.host)
    {
        return remote::run(args, common, &specific, target).await;
    }

    let runner = LocalRunner;
    crate::ui::print_banner();

    let existing_cp = checkpoint::load(&specific.install_path)?;

    // [1/10] 环境预检
    crate::ui::step_header("[1/10] 环境预检");
    if cpf::is_already_installed(&specific.install_path) {
        crate::ui::log_info(&format!(
            "检测到达梦数据库已安装至 {}，跳过安装",
            specific.install_path
        ));
        crate::ui::step_footer();
        return Ok(());
    }
    if existing_cp.is_some() {
        crate::ui::log_info("[续] 跳过预检查（从检查点续传）");
    } else {
        check_standalone_prerequisites(&runner, &specific).await?;
    }
    crate::ui::step_footer();

    // 初始化 checkpoint（提前，后续每步都基于它判断是否跳过）
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
        env_setup::run(&runner).await?;
        cp.env_setup_done = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [3/10] 下载安装包
    crate::ui::step_header("[3/10] 下载安装包");
    let extract_dir = if cp.installed {
        crate::ui::log_info("[续] dmdbms 已安装，跳过下载与解压");
        None
    } else {
        let package_path = resolve_package(args, &common.installer, &mut cp).await?;
        Some(step_extract(&package_path)?)
    };
    crate::ui::step_footer();

    // [4/10] 静默安装 dmdbms
    crate::ui::step_header("[4/10] 静默安装");
    if cp.installed {
        crate::ui::log_info(&format!(
            "[续] 跳过安装，dmdbms 已安装至 {}",
            specific.install_path
        ));
    } else {
        if Path::new(&specific.install_path)
            .join("bin/dminit")
            .exists()
        {
            anyhow::bail!(
                "安装目录 {} 已存在达梦数据库文件，请先卸载或修改 install_path",
                specific.install_path
            );
        }
        crate::ui::log_info("执行静默安装（解压 dmdbms 到安装目录）...");
        step_silent_install(
            &specific,
            extract_dir
                .as_ref()
                .expect("extract_dir set when !cp.installed"),
        )?;
        crate::ui::log_ok("安装完成");
        cp.installed = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [5/10] 初始化数据库
    crate::ui::step_header("[5/10] 初始化数据库");
    let db_already_inited = cp.db_inited
        || Path::new(&specific.data_path)
            .join("DAMENG/dm.ini")
            .exists();
    if db_already_inited {
        crate::ui::log_info("[续] 跳过 dminit，实例已初始化");
    } else {
        crate::ui::log_info("以 dmdba 用户初始化数据库实例...");
        init::run_dminit(&runner, &specific, &sysdba_pwd, &sysauditor_pwd).await?;
        crate::ui::log_ok("数据库初始化完成");
        cp.db_inited = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [6/10] 注册服务并启动
    crate::ui::step_header("[6/10] 注册服务");
    if cp.services_done {
        crate::ui::log_info("[续] 服务已注册，跳过");
    } else {
        service::register_and_start(&runner, &specific).await?;
        cp.services_done = true;
        cp.save()?;
    }
    service::wait_ready(&runner, &specific, &sysdba_pwd).await?;
    let dm_version = service::query_dm_version(&runner, &specific, &sysdba_pwd).await;
    crate::ui::step_footer();

    // [7/10] 配置归档（在线开启，dmserver 无需重启）
    crate::ui::step_header("[7/10] 配置归档");
    if cp.arch_configured {
        crate::ui::log_info("[续] 归档已配置，跳过");
    } else {
        archive::enable_archive_online(&runner, &specific, &sysdba_pwd).await?;
        cp.arch_configured = true;
        cp.save()?;
    }
    let arch_path = crate::config::resolve_arch_path(&specific.archive, &specific.data_path);
    crate::ui::log_ok(&format!("归档模式已配置: {}", arch_path));
    crate::ui::step_footer();

    // [8/10] 配置备份作业
    crate::ui::step_header("[8/10] 配置备份作业");
    if cp.backup_configured {
        crate::ui::log_info("[续] 备份作业已配置，跳过");
    } else {
        backup::configure_jobs(&runner, &specific, &sysdba_pwd).await?;
        cp.backup_configured = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [9/10] 开启 SQL 日志
    crate::ui::step_header("[9/10] 开启 SQL 日志");
    if cp.sql_log_enabled {
        crate::ui::log_info("[续] SQL 日志已开启，跳过");
    } else {
        sql_log::enable(&runner, &specific, &sysdba_pwd).await?;
        cp.sql_log_enabled = true;
        cp.save()?;
    }
    crate::ui::step_footer();

    // [10/10] 应用参数调优（执行 AutoParaAdj 脚本并重启 dmserver 使其生效）
    crate::ui::step_header("[10/10] 应用参数调优");
    if cp.param_tuned {
        crate::ui::log_info("[续] 参数调优已应用，跳过");
    } else {
        param_tune::apply_and_restart(&runner, &specific, &sysdba_pwd).await?;
        cp.param_tuned = true;
        cp.save()?;
        service::wait_ready(&runner, &specific, &sysdba_pwd).await?;
    }
    crate::ui::step_footer();

    crate::ui::print_success(
        &specific,
        &sysdba_pwd,
        &sysauditor_pwd,
        dm_version.as_deref(),
    );
    crate::ui::print_advisories(&crate::install::advisory::standalone_advisories(
        &specific, &arch_path,
    ));
    if let Some(cached) = &cp.package_cache {
        let _ = std::fs::remove_file(cached);
    }
    checkpoint::Checkpoint::remove()?;
    Ok(())
}

/// 判断 host 是否指向本机：localhost 别名 + 系统 hostname。
fn is_local_host(host: &str) -> bool {
    if matches!(host, "localhost" | "127.0.0.1" | "::1") {
        return true;
    }
    std::process::Command::new("hostname")
        .output()
        .map(|o| {
            let sys_hostname = String::from_utf8_lossy(&o.stdout);
            sys_hostname.trim() == host
        })
        .unwrap_or(false)
}

fn generate_passwords() -> (String, String) {
    (generate_password(), generate_password())
}

/// 生成满足达梦密码策略的随机密码（16 位，含大写/小写/数字，特殊字符仅用 _）。
/// 特殊字符只用 _ 避免 disql 连接串解析歧义（# 被截断为注释，@ 是 host 分隔符）。
pub(crate) fn generate_password() -> String {
    use rand::Rng;
    const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const LOWER: &[u8] = b"abcdefghjkmnpqrstuvwxyz";
    const DIGITS: &[u8] = b"23456789";
    const SPECIAL: &[u8] = b"_";
    const ALL: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789_";

    let mut rng = rand::thread_rng();
    let mut pwd: Vec<u8> = vec![
        UPPER[rng.gen_range(0..UPPER.len())],
        LOWER[rng.gen_range(0..LOWER.len())],
        DIGITS[rng.gen_range(0..DIGITS.len())],
        SPECIAL[rng.gen_range(0..SPECIAL.len())],
    ];
    for _ in 0..12 {
        pwd.push(ALL[rng.gen_range(0..ALL.len())]);
    }
    use rand::seq::SliceRandom;
    pwd.shuffle(&mut rng);
    String::from_utf8(pwd).expect("charset is ASCII")
}

/// 解析安装包路径：CLI > config > checkpoint 缓存 > 自动下载。
async fn resolve_package(
    args: &InstallArgs,
    installer: &crate::config::InstallerSource,
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
            let handle = crate::download::fetch_dm_installer().await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
    }
}

/// 将安装包复制到 CWD，文件名加 `.dm_cache_` 前缀，避免与用户文件冲突。
pub(crate) fn cache_package(src: &std::path::Path) -> Result<std::path::PathBuf> {
    let file_name = src
        .file_name()
        .map(|n| format!(".dm_cache_{}", n.to_string_lossy()))
        .unwrap_or_else(|| ".dm_cache_installer".to_string());
    let dest = std::env::current_dir()?.join(file_name);
    if dest.exists() {
        return Ok(dest);
    }
    std::fs::copy(src, &dest)
        .with_context(|| format!("缓存安装包失败: {} -> {}", src.display(), dest.display()))?;
    Ok(dest)
}

fn step_extract(path: &std::path::Path) -> Result<tempfile::TempDir> {
    package::extract_dminstall_bin(path)
}

fn step_silent_install(config: &InstallConfig, extract_dir: &tempfile::TempDir) -> Result<()> {
    let bin_path = extract_dir.path().join("DMInstall.bin");
    silent_install::install_from_bin(&bin_path, &config.install_path)
}

async fn check_standalone_prerequisites(
    runner: &LocalRunner,
    specific: &InstallConfig,
) -> Result<()> {
    cpf::check_memory(runner).await?;
    cpf::check_cpu_cores(runner).await?;
    cpf::check_disk_space(runner, &specific.install_path).await?;
    cpf::check_port_available(runner, specific.port).await?;
    cpf::check_port_available(runner, crate::config::AP_PORT_PRECHECK).await?;
    cpf::check_ulimits(runner).await?;
    cpf::check_selinux(runner).await?;
    Ok(())
}

#[cfg(test)]
fn validate_password_complexity(pwd: &str) -> Result<()> {
    if pwd.len() < 9 {
        anyhow::bail!("密码长度不足 9 位（达梦密码策略要求）");
    }
    let categories = [
        pwd.chars().any(|c| c.is_ascii_uppercase()),
        pwd.chars().any(|c| c.is_ascii_lowercase()),
        pwd.chars().any(|c| c.is_ascii_digit()),
        pwd.chars().any(|c| !c.is_alphanumeric()),
    ];
    if categories.iter().filter(|&&b| b).count() < 3 {
        anyhow::bail!("密码复杂度不足——需含大写字母、小写字母、数字、特殊字符中的至少三类");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_complexity_rejects_short() {
        assert!(validate_password_complexity("Ab1!").is_err());
    }

    #[test]
    fn test_password_complexity_rejects_simple() {
        assert!(validate_password_complexity("password123").is_err());
    }

    #[test]
    fn test_password_complexity_accepts_valid() {
        assert!(validate_password_complexity("DMAdmin1@2024").is_ok());
    }

    #[test]
    fn test_generate_password_length() {
        let pwd = generate_password();
        assert_eq!(pwd.len(), 16, "生成的密码应为 16 位");
    }

    #[test]
    fn test_generate_password_meets_complexity() {
        for _ in 0..20 {
            let pwd = generate_password();
            assert!(
                validate_password_complexity(&pwd).is_ok(),
                "生成的密码应满足复杂度: {}",
                pwd
            );
        }
    }

    #[tokio::test]
    async fn test_resolve_package_skips_download_when_cached() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cached_pkg = tmp.path().join(".dm_cache_dm8.iso");
        std::fs::write(&cached_pkg, b"fake iso").unwrap();

        let args = InstallArgs {
            package: None,
            url: None,
        };
        let mut cp = checkpoint::Checkpoint::new("/opt/dmdbms", "pwd1".into(), "pwd2".into());
        cp.package_cache = Some(cached_pkg.to_string_lossy().into_owned());

        // 若未命中缓存，Auto 会触发真实网络下载并在测试环境中报错/超时；
        // 命中缓存应直接返回缓存路径，不触碰网络。
        let resolved = resolve_package(&args, &crate::config::InstallerSource::Auto, &mut cp)
            .await
            .unwrap();
        assert_eq!(resolved, cached_pkg);
    }

    #[test]
    fn test_generate_password_is_ascii() {
        let pwd = generate_password();
        assert!(pwd.is_ascii(), "生成的密码应为纯 ASCII: {}", pwd);
    }

    #[test]
    fn test_generate_password_only_safe_special_chars() {
        for _ in 0..20 {
            let pwd = generate_password();
            let unsafe_chars: Vec<char> = pwd
                .chars()
                .filter(|&c| !c.is_alphanumeric() && c != '_')
                .collect();
            assert!(
                unsafe_chars.is_empty(),
                "密码只应含 _ 作为特殊字符，避免 disql 连接串解析问题: {:?} in {}",
                unsafe_chars,
                pwd
            );
        }
    }
}
