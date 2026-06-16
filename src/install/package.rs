use anyhow::{Context, Result};
use std::{path::Path, process::Command};
use tempfile::TempDir;

/// 从 DM 安装包提取 DMInstall.bin，返回含提取结果的临时目录。
/// 临时目录在返回的 TempDir drop 时自动清理。
/// 优先用 bsdtar（无需 root/loop 设备），失败或不可用时回退到 mount -o loop,ro。
pub fn extract_dminstall_bin(package_path: &Path) -> Result<TempDir> {
    let extract_dir = TempDir::new().context("创建临时目录失败")?;

    let name = package_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if name.ends_with("DMInstall.bin") {
        let dst = extract_dir.path().join("DMInstall.bin");
        std::fs::copy(package_path, &dst)
            .with_context(|| format!("复制 DMInstall.bin 失败: {}", package_path.display()))?;
        return Ok(extract_dir);
    }

    let bsdtar_available = Command::new("bsdtar").arg("--version").output().is_ok();
    if bsdtar_available {
        extract_via_bsdtar(package_path, &extract_dir)?;
    } else {
        extract_via_mount(package_path, &extract_dir)?;
    }
    Ok(extract_dir)
}

/// 通过 bsdtar 从 ISO 提取 DMInstall.bin（无需 root 或 loop 设备）。
fn extract_via_bsdtar(iso_path: &Path, extract_dir: &TempDir) -> Result<()> {
    let output = Command::new("bsdtar")
        .args(["-xf"])
        .arg(iso_path)
        .args(["--include", "*DMInstall.bin"])
        .arg("-C")
        .arg(extract_dir.path())
        .output()
        .context("执行 bsdtar 失败")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "bsdtar 提取 ISO {} 失败: {}",
            iso_path.display(),
            stderr.trim()
        );
    }

    // DMInstall.bin 可能在子目录中，移到 extract_dir 根
    let dst = extract_dir.path().join("DMInstall.bin");
    if !dst.exists() {
        let found = find_dminstall_bin(extract_dir.path()).with_context(|| {
            format!(
                "bsdtar 未在安装包中找到 DMInstall.bin: {}",
                iso_path.display()
            )
        })?;
        std::fs::rename(&found, &dst)
            .or_else(|_| std::fs::copy(&found, &dst).map(|_| ()))
            .with_context(|| format!("移动 DMInstall.bin 失败: {}", found.display()))?;
    }
    Ok(())
}

/// 通过 mount -o loop,ro 挂载 ISO，find DMInstall.bin 后复制出来。
fn extract_via_mount(iso_path: &Path, extract_dir: &TempDir) -> Result<()> {
    let mount_point = TempDir::new().context("创建挂载点失败")?;

    let output = Command::new("mount")
        .args(["-o", "loop,ro"])
        .arg(iso_path)
        .arg(mount_point.path())
        .output()
        .context("执行 mount 失败")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "mount -o loop,ro {} {} 失败: {}",
            iso_path.display(),
            mount_point.path().display(),
            stderr.trim()
        );
    }

    let src = find_dminstall_bin(mount_point.path());
    let _ = Command::new("umount").arg(mount_point.path()).status();

    let src =
        src.with_context(|| format!("未在安装包中找到 DMInstall.bin: {}", iso_path.display()))?;
    let dst = extract_dir.path().join("DMInstall.bin");
    std::fs::copy(&src, &dst)
        .with_context(|| format!("复制 DMInstall.bin 失败: {}", src.display()))?;
    Ok(())
}

/// 在目录树中递归查找第一个 DMInstall.bin。
fn find_dminstall_bin(dir: &Path) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_dminstall_bin(&path) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some("DMInstall.bin") {
            return Some(path);
        }
    }
    None
}
