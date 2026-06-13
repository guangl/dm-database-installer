use anyhow::{Result, bail};

use crate::cli::InstallArgs;
use crate::config::InstallConfig;

pub mod checksum;
pub mod idempotent;
pub mod init;
pub mod package;
pub mod silent_install;

/// 根据 InstallArgs 加载配置，--config 为必填项。
fn resolve_config(args: &InstallArgs) -> Result<InstallConfig> {
    match &args.config {
        Some(path) => crate::config::load_and_validate(path),
        None => bail!(
            "安装前需要配置文件\n\
             请先运行:\n\
             \n  dm-installer init standalone\n\
             \n然后编辑生成的 dm-standalone.toml，再用 --config 指定"
        ),
    }
}

/// 安装子命令入口。
///
/// 流程：幂等检测 → 密码输入 → 包路径 → checksum → ISO 提取 → DMInstall.bin → dminit
pub async fn run(args: &InstallArgs) -> Result<()> {
    tracing::info!("开始安装达梦数据库");
    let config = resolve_config(args)?;

    if check_idempotent_early_exit(&config)? {
        return Ok(());
    }

    let (sysdba_pwd, sysauditor_pwd) = prompt_passwords()?;

    let package = fetch_package(args).await?;
    verify_checksum(args, &package.path)?;

    let extract_dir = step_extract(&package.path)?;
    step_silent_install(&config, &extract_dir)?;
    step_dminit(&config, &sysdba_pwd, &sysauditor_pwd)?;

    tracing::info!("Plan 04 将注册 systemd 服务");
    Ok(())
}

fn check_idempotent_early_exit(config: &InstallConfig) -> Result<bool> {
    tracing::info!("[1/7] 幂等性检测");
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
        bail!("密码长度不足 9 位（达梦密码策略要求）");
    }
    let categories = [
        pwd.chars().any(|c| c.is_ascii_uppercase()),
        pwd.chars().any(|c| c.is_ascii_lowercase()),
        pwd.chars().any(|c| c.is_ascii_digit()),
        pwd.chars().any(|c| !c.is_alphanumeric()),
    ];
    if categories.iter().filter(|&&b| b).count() < 3 {
        bail!("密码复杂度不足——需含大写字母、小写字母、数字、特殊字符中的至少三类");
    }
    Ok(())
}

async fn fetch_package(args: &InstallArgs) -> Result<crate::common::download::PackageHandle> {
    tracing::info!("[2/7] 获取安装包路径");
    match &args.package {
        Some(p) => {
            println!("使用本地安装包: {}", p.display());
            Ok(crate::common::download::PackageHandle::from_user_path(p.clone()))
        }
        None => crate::common::download::fetch_dm_installer().await,
    }
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
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config_from_args_requires_config_file() {
        let args = InstallArgs { package: None, checksum: None, config: None };
        let err = resolve_config(&args).unwrap_err();
        assert!(format!("{err}").contains("dm-installer init standalone"));
    }

    #[test]
    fn test_load_config_from_args_uses_file_when_some() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, concat!(
            "port = 5237\n",
            "page_size = 16\n",
            "charset = 1\n",
            "extent_size = 32\n",
            "install_path = \"/opt/dmdbms\"\n",
            "data_path = \"/opt/dmdbms/data\"\n",
            "instance_name = \"DMSERVER\"\n",
            "case_sensitive = true\n",
        )).unwrap();
        let args = InstallArgs {
            package: None,
            checksum: None,
            config: Some(file.path().to_path_buf()),
        };
        let cfg = resolve_config(&args).expect("应返回 Ok(InstallConfig)");
        assert_eq!(cfg.port, 5237, "port 应为 5237");
        assert_eq!(cfg.page_size, 16, "page_size 应为 16");
        assert_eq!(cfg.charset, 1, "charset 应为 1");
        assert_eq!(cfg.extent_size, 32, "extent_size 应为 32");
    }

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
