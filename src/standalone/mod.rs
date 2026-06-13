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

    let (sysdba_pwd, sysauditor_pwd) = prompt_passwords()?;

    let package = fetch_package(args, &common).await?;
    verify_checksum(args, &package.path)?;

    let extract_dir = step_extract(&package.path)?;
    step_silent_install(&specific, &extract_dir)?;
    step_dminit(&specific, &sysdba_pwd, &sysauditor_pwd)?;

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

/// 从终端提示输入并确认 SYSDBA / SYSAUDITOR 密码。密码不写入任何文件。
fn prompt_passwords() -> Result<(String, String)> {
    println!("[密码设置] 请输入达梦管理员密码（不回显）");
    println!("           要求：至少 9 位，含大写/小写/数字/特殊字符中的至少三类");
    let sysdba_pwd = prompt_and_confirm("SYSDBA 管理员")?;
    let sysauditor_pwd = prompt_and_confirm("SYSAUDITOR 审计员")?;
    Ok((sysdba_pwd, sysauditor_pwd))
}

fn prompt_and_confirm(role: &str) -> Result<String> {
    loop {
        let pwd = rpassword::prompt_password(format!("  {} 密码: ", role))
            .map_err(|e| anyhow::anyhow!("读取密码失败: {e}"))?;
        if let Err(e) = validate_password_complexity(&pwd) {
            eprintln!("  [错误] {e}");
            continue;
        }
        let confirm = rpassword::prompt_password(format!("  {} 密码（确认）: ", role))
            .map_err(|e| anyhow::anyhow!("读取密码失败: {e}"))?;
        if pwd == confirm {
            return Ok(pwd);
        }
        eprintln!("  [错误] 两次输入不一致，请重新输入");
    }
}

/// 达梦密码复杂度：至少 9 位，含大写/小写/数字/特殊字符中的至少三类。
pub(crate) fn validate_password_complexity(pwd: &str) -> Result<()> {
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
    tracing::info!("[5/6] DMInstall.bin 静默安装");
    silent_install::run(config, extract_dir.path())
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
}
