use anyhow::Result;

use crate::cli::InstallArgs;

pub mod checksum;
pub mod idempotent;

/// 安装子命令入口（Plan 02 编排：幂等检测 → 包路径 → checksum 校验）。
///
/// 剩余步骤由后续 Plan 填充：
/// - Plan 03: ISO 提取 + 参数确认 + DMInstall.bin 调用
/// - Plan 04: systemd 服务注册
pub async fn run(_args: &InstallArgs) -> Result<()> {
    // Plan 02 将替换为完整安装编排流程
    Ok(())
}
