use anyhow::Result;

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
        None => anyhow::bail!(
            "安装前需要配置文件（包含 sysdba_pwd / sysauditor_pwd 等必填项）\n\
             请先运行:\n\
             \n  dm-installer init standalone\n\
             \n然后编辑生成的 dm-standalone.toml，再用 --config 指定"
        ),
    }
}

/// 安装子命令入口（INST-01 完整编排器）。
///
/// 流程：幂等检测 → 包路径 → checksum → ISO 提取 → DMInstall.bin → dminit
pub async fn run(args: &InstallArgs) -> Result<()> {
    tracing::info!("开始安装达梦数据库");
    let config = resolve_config(args)?;

    if check_idempotent_early_exit(&config)? {
        return Ok(());
    }

    let package = fetch_package(args).await?;
    verify_checksum(args, &package.path)?;

    let extract_dir = step_extract(&package.path)?;
    step_silent_install(&config, &extract_dir)?;
    step_dminit(&config)?;

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

fn step_dminit(config: &InstallConfig) -> Result<()> {
    tracing::info!("[6/6] dminit 初始化");
    init::run_dminit(config)
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
            "sysdba_pwd = \"DMAdmin1@2024\"\n",
            "sysauditor_pwd = \"AuditAdmin2#2024\"\n",
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
}
