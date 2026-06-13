use anyhow::Result;

use crate::cli::InstallArgs;
use crate::config::{CommonConfig, InstallConfig};

pub mod checksum;
pub mod idempotent;
pub mod init;
pub mod package;
pub mod remote;
pub mod silent_install;

/// 单机安装入口。common 和 specific 已由调用方从配置文件加载并验证。
pub async fn run(args: &InstallArgs, common: CommonConfig, specific: InstallConfig) -> Result<()> {
    if let Some(target) = &specific.ssh_target {
        if is_local_host(&target.host) {
            tracing::info!("ssh_target.host 为本机，跳过 SSH，执行本地安装");
        } else {
            return remote::run(args, common, &specific, target).await;
        }
    }

    tracing::info!("开始安装达梦数据库（单机）");

    if check_idempotent_early_exit(&specific)? {
        return Ok(());
    }

    let (sysdba_pwd, sysauditor_pwd) = generate_passwords();

    let package = fetch_package(args, &common).await?;
    verify_checksum(args, &package.path)?;

    let extract_dir = step_extract(&package.path)?;
    step_silent_install(&specific, &extract_dir)?;
    step_dminit(&specific, &sysdba_pwd, &sysauditor_pwd)?;

    print_generated_credentials(&sysdba_pwd, &sysauditor_pwd);
    tracing::info!("单机安装完成");
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

fn check_idempotent_early_exit(config: &InstallConfig) -> Result<bool> {
    tracing::info!("[1/6] 幂等性检测");
    if idempotent::check_existing_instance(config)? {
        println!("已检测到达梦实例 ({}/dm.ini)，跳过安装", config.install_path);
        return Ok(true);
    }
    Ok(false)
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

fn print_generated_credentials(sysdba_pwd: &str, sysauditor_pwd: &str) {
    println!();
    println!("╔══════════════════════════════════════════════════╗");
    println!("║              达梦数据库初始凭证                  ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  SYSDBA    密码: {:<33}║", sysdba_pwd);
    println!("║  SYSAUDITOR密码: {:<33}║", sysauditor_pwd);
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  首次登录后请立即修改密码                        ║");
    println!("╚══════════════════════════════════════════════════╝");
    println!();
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

async fn fetch_package(args: &InstallArgs, common: &CommonConfig) -> Result<crate::common::download::PackageHandle> {
    tracing::info!("[2/6] 获取安装包路径");
    // CLI --package > config.toml installer_package > 自动下载
    if let Some(p) = &args.package {
        println!("使用本地安装包 (CLI): {}", p.display());
        return Ok(crate::common::download::PackageHandle::from_user_path(p.clone()));
    }
    if let Some(p) = &common.installer_package {
        println!("使用本地安装包 (config.toml): {}", p.display());
        return Ok(crate::common::download::PackageHandle::from_user_path(p.clone()));
    }
    crate::common::download::fetch_dm_installer().await
}

fn verify_checksum(args: &InstallArgs, path: &std::path::Path) -> Result<()> {
    tracing::info!("[3/6] SHA-256 校验");
    if let Some(expected) = &args.checksum {
        checksum::verify_sha256(path, expected)
    } else {
        tracing::warn!("未提供 --checksum，跳过 SHA-256 校验");
        Ok(())
    }
}

fn step_extract(path: &std::path::Path) -> Result<tempfile::TempDir> {
    tracing::info!("[4/6] 提取 DMInstall.bin");
    package::extract_dminstall_bin(path)
}

fn step_silent_install(config: &InstallConfig, extract_dir: &tempfile::TempDir) -> Result<()> {
    tracing::info!("[5/6] 安装 dmdbms");
    let bin_path = extract_dir.path().join("DMInstall.bin");
    silent_install::install_from_bin(&bin_path, &config.install_path)
}

fn step_dminit(config: &InstallConfig, sysdba_pwd: &str, sysauditor_pwd: &str) -> Result<()> {
    tracing::info!("[6/6] dminit 初始化");
    init::run_dminit(config, sysdba_pwd, sysauditor_pwd)
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
