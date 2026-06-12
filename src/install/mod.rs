use anyhow::Result;

use crate::cli::InstallArgs;

/// 安装子命令入口占位（Phase 1 骨架）。
///
/// 完整实现在 Plan 02-04 中逐步填充：
/// - Plan 02: checksum 验证 + package 提取
/// - Plan 03: 参数确认 + DMInstall.bin 调用
/// - Plan 04: systemd 服务注册
pub async fn run(_args: &InstallArgs) -> Result<()> {
    // Phase 1 骨架：直接返回 Ok(())
    // Plan 02 将替换为完整安装编排流程
    Ok(())
}
