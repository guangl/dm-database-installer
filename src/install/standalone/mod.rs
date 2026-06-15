use anyhow::{Context, Result};
use std::path::Path;

use crate::cli::InstallArgs;
use crate::install::{checksum, env_setup, package, preflight as cpf, silent_install};
use crate::ssh::LocalRunner;
use crate::config::{CommonConfig, InstallConfig};

pub mod checkpoint;
pub mod init;
pub mod remote;
pub mod rollback;
pub mod service;

/// 单机安装入口。common 和 specific 已由调用方从配置文件加载并验证。
pub async fn run(args: &InstallArgs, common: CommonConfig, specific: InstallConfig) -> Result<()> {
    if let Some(target) = &specific.ssh_target {
        if is_local_host(&target.host) {
            tracing::info!("ssh_target.host 为本机，跳过 SSH，执行本地安装");
        } else {
            return remote::run(args, common, &specific, target).await;
        }
    }

    crate::ui::print_banner();

    // [1/6] 环境预检
    crate::ui::step_header("[1/6] 环境预检");
    check_standalone_prerequisites(&specific.install_path, specific.port).await?;
    crate::ui::step_footer();

    let mut rb = rollback::StandaloneRollback::new(
        &specific.install_path,
        &specific.data_path,
        &specific.instance_name,
    );

    // [2/6] 系统准备
    crate::ui::step_header("[2/6] 系统准备");
    let env_backup = rollback::EnvBackup::capture()?;
    env_setup::run(&LocalRunner).await?;
    rb.set_env_backup(env_backup);
    let existing_cp = checkpoint::load(&specific.install_path)?;
    let (sysdba_pwd, sysauditor_pwd) = match &existing_cp {
        Some(c) => (c.sysdba_pwd.clone(), c.sysauditor_pwd.clone()),
        None => generate_passwords(),
    };
    let mut cp = existing_cp.unwrap_or_else(|| {
        checkpoint::Checkpoint::new(&specific.install_path, sysdba_pwd.clone(), sysauditor_pwd.clone())
    });
    cp.save()?;
    crate::ui::step_footer();

    // [3/6] 下载安装包
    crate::ui::step_header("[3/6] 下载安装包");
    let package_path = resolve_package(args, &common.installer, &mut cp).await?;
    verify_checksum(args, &package_path)?;
    let extract_dir = step_extract(&package_path)?;
    crate::ui::step_footer();

    // [4/6] 静默安装 dmdbms
    crate::ui::step_header("[4/6] 静默安装");
    let bin_installed = Path::new(&specific.install_path).join("bin/dminit").exists();
    if cp.installed || bin_installed {
        crate::ui::log_info(&format!("[续] 跳过安装，dmdbms 已安装至 {}", specific.install_path));
    } else {
        crate::ui::log_info("执行静默安装（以下为安装器输出）...");
        step_silent_install(&specific, &extract_dir)?;
        crate::ui::log_ok("安装完成");
        cp.installed = true;
        cp.save()?;
    }
    rb.installed = true;
    crate::ui::step_footer();

    // [5/6] 初始化数据库（幂等：dm.ini 存在则跳过）
    crate::ui::step_header("[5/6] 初始化数据库");
    let dm_ini_exists = Path::new(&specific.data_path).join("DAMENG/dm.ini").exists();
    if dm_ini_exists {
        crate::ui::log_info("[续] 跳过 dminit，实例已初始化");
    } else {
        crate::ui::log_info("以 dmdba 用户初始化数据库实例...");
        step_dminit(&specific, &sysdba_pwd, &sysauditor_pwd)?;
        crate::ui::log_ok("数据库初始化完成");
        step_write_dmarch_ini(&specific)?;
    }
    rb.db_inited = true;
    crate::ui::step_footer();

    // [6/6] 注册服务 + 开启归档
    crate::ui::step_header("[6/6] 注册服务");
    step_register_service(&specific)?;
    rb.services_registered = true;
    crate::ui::log_info("等待数据库就绪...");
    step_enable_archivelog(&specific, &sysdba_pwd)?;
    let arch_path = crate::config::resolve_arch_path(&specific.archive, &specific.data_path);
    crate::ui::log_ok(&format!("归档模式已开启: {}", arch_path));
    crate::ui::step_footer();

    crate::ui::print_success(&specific, &sysdba_pwd, &sysauditor_pwd);
    if let Some(cached) = &cp.package_cache {
        let _ = std::fs::remove_file(cached);
    }
    checkpoint::Checkpoint::remove()?;
    tracing::debug!("单机安装完成");
    rb.commit();
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

/// 生成满足达梦密码策略的随机密码（16 位，含大写/小写/数字/特殊字符）。
pub(crate) fn generate_password() -> String {
    use rand::Rng;
    const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const LOWER: &[u8] = b"abcdefghjkmnpqrstuvwxyz";
    const DIGITS: &[u8] = b"23456789";
    const SPECIAL: &[u8] = b"@#$%&*!";
    const ALL: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789@#$%&*!";

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

/// 解析安装包路径：CLI > config > checkpoint 缓存 > 自动下载。
/// 需要网络下载时将结果持久化到 CWD（.dm_cache_ 前缀），支持中断续传。
async fn resolve_package(
    args: &InstallArgs,
    installer: &crate::config::InstallerSource,
    cp: &mut checkpoint::Checkpoint,
) -> Result<std::path::PathBuf> {
    use crate::config::InstallerSource;
    tracing::info!("[2/7] 获取安装包路径");

    if let Some(p) = &args.package {
        crate::ui::log_info(&format!("使用本地安装包 (CLI --package): {}", p.display()));
        return Ok(p.clone());
    }
    if let Some(url) = &args.url {
        crate::ui::log_info(&format!("下载安装包 (CLI --url): {}", url));
        let handle = crate::download::fetch_from_url(url).await?;
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
            if let Some(cached) = cp.package_cache.as_ref().map(std::path::Path::new).filter(|p| p.exists()) {
                crate::ui::log_info(&format!("[续] 跳过下载，使用已缓存安装包: {}", cached.display()));
                return Ok(cached.to_path_buf());
            }
            let handle = crate::download::fetch_from_url(url).await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
        InstallerSource::Auto => {
            if let Some(cached) = cp.package_cache.as_ref().map(std::path::Path::new).filter(|p| p.exists()) {
                crate::ui::log_info(&format!("[续] 跳过下载，使用已缓存安装包: {}", cached.display()));
                return Ok(cached.to_path_buf());
            }
            let handle = crate::download::fetch_dm_installer().await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
    }
}

/// 将安装包复制到 CWD，文件名加 `.dm_cache_` 前缀，避免与用户文件冲突。
fn cache_package(src: &std::path::Path) -> Result<std::path::PathBuf> {
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

fn verify_checksum(args: &InstallArgs, path: &std::path::Path) -> Result<()> {
    tracing::info!("[3/7] SHA-256 校验");
    if let Some(expected) = &args.checksum {
        checksum::verify_sha256(path, expected)
    } else {
        tracing::warn!("未提供 --checksum，跳过 SHA-256 校验");
        Ok(())
    }
}

fn step_extract(path: &std::path::Path) -> Result<tempfile::TempDir> {
    tracing::info!("[4/7] 提取 DMInstall.bin");
    package::extract_dminstall_bin(path)
}

fn step_silent_install(config: &InstallConfig, extract_dir: &tempfile::TempDir) -> Result<()> {
    tracing::info!("[5/7] 安装 dmdbms");
    let bin_path = extract_dir.path().join("DMInstall.bin");
    silent_install::install_from_bin(&bin_path, &config.install_path)
}

fn step_dminit(config: &InstallConfig, sysdba_pwd: &str, sysauditor_pwd: &str) -> Result<()> {
    tracing::info!("[6/7] dminit 初始化");
    init::run_dminit(config, sysdba_pwd, sysauditor_pwd)
}

fn step_write_dmarch_ini(config: &InstallConfig) -> Result<()> {
    tracing::info!("[6b/7] 写入 dmarch.ini");
    init::write_dmarch_ini(config)
}

fn step_register_service(config: &InstallConfig) -> Result<()> {
    tracing::info!("[7/7] 注册并启动 DM 服务");
    service::register_and_start(config)
}

fn step_enable_archivelog(config: &InstallConfig, sysdba_pwd: &str) -> Result<()> {
    tracing::info!("[7b/7] 开启归档模式");
    init::enable_archivelog(config, sysdba_pwd)
}

async fn check_standalone_prerequisites(install_path: &str, port: u16) -> Result<()> {
    tracing::info!("[预检查] 本地硬件资源检测");
    let runner = LocalRunner;
    cpf::check_memory(&runner).await?;
    cpf::check_cpu_cores(&runner).await?;
    cpf::check_disk_space(&runner, install_path).await?;
    cpf::check_port_available(&runner, port).await?;
    cpf::check_ulimits(&runner).await?;
    cpf::check_selinux(&runner).await?;
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
            assert!(validate_password_complexity(&pwd).is_ok(), "生成的密码应满足复杂度: {}", pwd);
        }
    }

    #[test]
    fn test_generate_password_is_ascii() {
        let pwd = generate_password();
        assert!(pwd.is_ascii(), "生成的密码应为纯 ASCII: {}", pwd);
    }
}
