use anyhow::Result;

use crate::cli::InstallArgs;
use crate::config::InstallConfig;
use crate::ui::StatusLevel;

pub mod checksum;
pub mod idempotent;
pub mod init;
pub mod package;
pub mod silent_install;

/// 安装子命令入口（INST-01 完整编排器）。
///
/// 流程：幂等检测 → 包路径 → checksum → ISO 提取 → 参数确认 → DMInstall.bin → dminit
/// Plan 04 接管：systemd 服务注册
pub async fn run(args: &InstallArgs) -> Result<()> {
    tracing::info!("开始安装达梦数据库");
    let config = InstallConfig::default();

    if check_idempotent_early_exit(&config)? {
        return Ok(());
    }

    let iso_path = fetch_package(args).await?;
    verify_checksum(args, &iso_path)?;

    let extract_dir = step_extract(&iso_path)?;
    step_confirm_params(args, &config)?;
    step_silent_install(&config, &extract_dir)?;
    step_dminit(&config)?;

    crate::ui::print_status(StatusLevel::Info, "Plan 04 将注册 systemd 服务");
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

async fn fetch_package(args: &InstallArgs) -> Result<std::path::PathBuf> {
    tracing::info!("[2/7] 获取安装包路径");
    match &args.package {
        Some(p) => Ok(p.clone()),
        None => crate::download::fetch_dm_installer().await,
    }
}

fn verify_checksum(args: &InstallArgs, iso_path: &std::path::Path) -> Result<()> {
    tracing::info!("[3/7] SHA-256 校验");
    if let Some(expected) = &args.checksum {
        checksum::verify_sha256(iso_path, expected)
    } else {
        tracing::warn!("未提供 --checksum，跳过 SHA-256 校验");
        Ok(())
    }
}

fn step_extract(iso_path: &std::path::Path) -> Result<tempfile::TempDir> {
    tracing::info!("[4/7] 提取 DMInstall.bin");
    package::extract_dminstall_bin(iso_path)
}

fn step_confirm_params(args: &InstallArgs, config: &InstallConfig) -> Result<()> {
    tracing::info!("[5/7] 参数确认");
    crate::ui::confirm_immutable_params(config, args.defaults || args.yes)
}

fn step_silent_install(config: &InstallConfig, extract_dir: &tempfile::TempDir) -> Result<()> {
    tracing::info!("[6/7] DMInstall.bin 静默安装");
    silent_install::run(config, extract_dir.path())
}

fn step_dminit(config: &InstallConfig) -> Result<()> {
    tracing::info!("[7/7] dminit 初始化");
    init::run_dminit(config)
}
