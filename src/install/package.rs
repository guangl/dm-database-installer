use anyhow::{Context, Result};
use std::{path::Path, process::Command};
use tempfile::TempDir;

/// 从 DM ISO 包提取 DMInstall.bin，返回含提取结果的临时目录。
///
/// 策略 A: bsdtar（无需 root，优先使用）
/// 策略 B: mount -o loop（需 root，安装器已以 root 运行）
/// 临时目录在返回的 TempDir drop 时自动清理。
pub fn extract_dminstall_bin(iso_path: &Path) -> Result<TempDir> {
    todo!("Task 2 RED: 待实现 extract_dminstall_bin")
}

fn is_command_available(cmd: &str) -> bool {
    todo!("Task 2 RED: 待实现 is_command_available")
}

fn extract_via_mount(iso_path: &Path, extract_dir: &TempDir) -> Result<()> {
    todo!("Task 2 RED: 待实现 extract_via_mount")
}
