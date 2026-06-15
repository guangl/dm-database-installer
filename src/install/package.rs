use anyhow::{Context, Result};
use std::{path::Path, process::Command};
use tempfile::TempDir;

/// 从 DM ISO 包提取 DMInstall.bin，返回含提取结果的临时目录。
///
/// 策略 A: bsdtar（无需 root，优先使用）
/// 策略 B: mount -o loop（需 root，安装器已以 root 运行）
/// 临时目录在返回的 TempDir drop 时自动清理。
pub fn extract_dminstall_bin(iso_path: &Path) -> Result<TempDir> {
    tracing::info!("提取 DMInstall.bin: {}", iso_path.display());
    let extract_dir = TempDir::new().context("创建临时目录失败")?;
    tracing::debug!("临时目录: {}", extract_dir.path().display());

    // 策略 A: bsdtar（无需 root，优先使用）
    if is_command_available("bsdtar") {
        tracing::debug!("使用策略 A: bsdtar");
        let status = Command::new("bsdtar")
            .args(["x", "-f"])
            .arg(iso_path)
            .arg("-C")
            .arg(extract_dir.path())
            .status()
            .context("执行 bsdtar 失败")?;
        if status.success() {
            tracing::debug!("bsdtar 提取成功");
            return Ok(extract_dir);
        }
        tracing::warn!("bsdtar 执行失败，fallback 到 mount -o loop");
    } else {
        tracing::warn!("bsdtar 不可用，fallback 到 mount -o loop（Pitfall 3）");
    }

    // 策略 B: mount -o loop（安装器已以 root 运行）
    tracing::debug!("使用策略 B: mount -o loop");
    extract_via_mount(iso_path, &extract_dir)?;
    Ok(extract_dir)
}

/// 通过 mount -o loop 提取 DMInstall.bin。
fn extract_via_mount(iso_path: &Path, extract_dir: &TempDir) -> Result<()> {
    let mount_point = TempDir::new().context("创建挂载点失败")?;
    tracing::debug!("mount -o loop {} -> {}", iso_path.display(), mount_point.path().display());

    let status = Command::new("mount")
        .args(["-o", "loop"])
        .arg(iso_path)
        .arg(mount_point.path())
        .status()
        .context("mount -o loop 失败，请确认以 root 运行")?;
    anyhow::ensure!(status.success(), "mount 返回非零退出码");
    tracing::debug!("mount 成功");

    let src = mount_point.path().join("DMInstall.bin");
    let dst = extract_dir.path().join("DMInstall.bin");
    let bytes_copied = std::fs::copy(&src, &dst).with_context(|| {
        format!(
            "复制 DMInstall.bin 失败，检查 ISO 内容: {}",
            src.display()
        )
    })?;
    tracing::debug!("复制 DMInstall.bin 完成: {} bytes", bytes_copied);

    let umount_status = Command::new("umount").arg(mount_point.path()).status();
    match umount_status {
        Ok(s) if s.success() => tracing::debug!("umount 成功"),
        Ok(s) => tracing::warn!("umount 返回非零退出码: {:?}", s.code()),
        Err(e) => tracing::warn!("umount 执行失败: {}", e),
    }
    Ok(())
}

/// 检测外部命令是否可用。
fn is_command_available(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
