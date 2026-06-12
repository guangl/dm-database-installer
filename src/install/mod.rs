use anyhow::Result;

use crate::cli::InstallArgs;
use crate::config::InstallConfig;

pub mod checksum;
pub mod idempotent;

/// 安装子命令入口（Plan 02 编排：幂等检测 → 包路径 → checksum 校验）。
///
/// 剩余步骤由后续 Plan 填充：
/// - Plan 03: ISO 提取 + 参数确认 + DMInstall.bin 调用
/// - Plan 04: systemd 服务注册
pub async fn run(args: &InstallArgs) -> Result<()> {
    let config = InstallConfig::default();

    // Step 1: 幂等检测（D-08）
    if idempotent::check_existing_instance(&config)? {
        println!(
            "已检测到达梦实例 ({}/dm.ini)，跳过安装",
            config.install_path
        );
        return Ok(());
    }

    // Step 2: 获取安装包路径
    let iso_path = match &args.package {
        Some(p) => p.clone(),
        None => crate::download::fetch_dm_installer().await?,
    };

    // Step 3: SHA-256 校验（DOWN-02）
    if let Some(expected) = &args.checksum {
        checksum::verify_sha256(&iso_path, expected)?;
    } else {
        tracing::warn!("未提供 --checksum，跳过 SHA-256 校验");
    }

    // TODO Plan 03: ISO 提取
    // TODO Plan 03: 参数确认（ui::confirm_immutable_params）
    // TODO Plan 03: DMInstall.bin -q <xml> 调用
    // TODO Plan 03: dminit 初始化
    // TODO Plan 04: systemd 服务注册
    Ok(())
}
