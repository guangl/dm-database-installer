use anyhow::Result;

use crate::cli::InstallArgs;
use crate::config::InstallConfig;

pub mod checksum;
pub mod idempotent;
pub mod init;
pub mod package;
pub mod silent_install;

/// 根据 InstallArgs 决定配置来源：有 --config 则从文件加载，否则使用默认值。
fn resolve_config(args: &InstallArgs) -> Result<InstallConfig> {
    match &args.config {
        Some(path) => crate::config::load_and_validate(path),
        None => Ok(InstallConfig::default()),
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
        Some(p) => Ok(crate::common::download::PackageHandle::from_user_path(p.clone())),
        None => crate::common::download::fetch_dm_installer(args.defaults || args.yes).await,
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

    fn make_args_no_config() -> InstallArgs {
        InstallArgs {
            package: None,
            checksum: None,
            defaults: false,
            yes: false,
            config: None,
        }
    }

    #[test]
    fn test_load_config_from_args_uses_default_when_none() {
        let args = make_args_no_config();
        let cfg = resolve_config(&args).expect("应返回 Ok(InstallConfig)");
        let default = InstallConfig::default();
        assert_eq!(cfg.port, default.port, "port 应与默认值相同");
        assert_eq!(cfg.page_size, default.page_size, "page_size 应与默认值相同");
        assert_eq!(cfg.charset, default.charset, "charset 应与默认值相同");
        assert_eq!(cfg.extent_size, default.extent_size, "extent_size 应与默认值相同");
        assert_eq!(cfg.install_path, default.install_path, "install_path 应与默认值相同");
        assert_eq!(cfg.data_path, default.data_path, "data_path 应与默认值相同");
        assert_eq!(cfg.instance_name, default.instance_name, "instance_name 应与默认值相同");
        assert_eq!(cfg.case_sensitive, default.case_sensitive, "case_sensitive 应与默认值相同");
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
            defaults: false,
            yes: false,
            config: Some(file.path().to_path_buf()),
        };
        let cfg = resolve_config(&args).expect("应返回 Ok(InstallConfig)");
        assert_eq!(cfg.port, 5237, "port 应为 5237");
        assert_eq!(cfg.page_size, 16, "page_size 应为 16");
        assert_eq!(cfg.charset, 1, "charset 应为 1");
        assert_eq!(cfg.extent_size, 32, "extent_size 应为 32");
    }
}
