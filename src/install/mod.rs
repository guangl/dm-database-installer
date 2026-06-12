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

    // Step 1: 幂等检测（D-08）
    tracing::info!("[1/7] 幂等性检测");
    if idempotent::check_existing_instance(&config)? {
        println!(
            "已检测到达梦实例 ({}/dm.ini)，跳过安装",
            config.install_path
        );
        return Ok(());
    }

    // Step 2: 获取安装包路径
    tracing::info!("[2/7] 获取安装包路径");
    let iso_path = match &args.package {
        Some(p) => p.clone(),
        None => crate::download::fetch_dm_installer().await?,
    };

    // Step 3: SHA-256 校验（DOWN-02）
    tracing::info!("[3/7] SHA-256 校验");
    if let Some(expected) = &args.checksum {
        checksum::verify_sha256(&iso_path, expected)?;
    } else {
        tracing::warn!("未提供 --checksum，跳过 SHA-256 校验");
    }

    // Step 4: 提取 DMInstall.bin（Pitfall 3: bsdtar + mount fallback）
    tracing::info!("[4/7] 提取 DMInstall.bin");
    let extract_dir = package::extract_dminstall_bin(&iso_path)?;

    // Step 5: 确认不可修改参数（INST-03）
    tracing::info!("[5/7] 参数确认");
    let skip_confirm = args.defaults || args.yes;
    crate::ui::confirm_immutable_params(&config, skip_confirm)?;

    // Step 6: DMInstall.bin -q 静默安装
    tracing::info!("[6/7] DMInstall.bin 静默安装");
    silent_install::run(&config, extract_dir.path())?;

    // Step 7: dminit 初始化数据库实例
    tracing::info!("[7/7] dminit 初始化");
    init::run_dminit(&config)?;

    // TODO Plan 04: service::register_systemd_service(&config)?;
    crate::ui::print_status(StatusLevel::Info, "Plan 04 将注册 systemd 服务");

    Ok(())
}
