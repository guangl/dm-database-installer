use anyhow::{Context, Result};
use std::{path::Path, process::Command};
use tempfile::TempDir;

/// 将 DMInstall.bin (ZIP格式) 解压，把 dmdbms 目录复制到 install_path。
pub fn install_from_bin(bin_path: &Path, install_path: &str) -> Result<()> {
    let tmp = TempDir::new().context("创建临时目录失败")?;

    let status = Command::new("unzip")
        .arg("-q")
        .arg(bin_path)
        .arg("-d")
        .arg(tmp.path())
        .status()
        .context("执行 unzip 失败，请确认 unzip 已安装")?;
    anyhow::ensure!(status.success(), "unzip DMInstall.bin 返回非零退出码");

    let dmdbms_src = tmp.path().join("dmdbms");
    anyhow::ensure!(
        dmdbms_src.exists(),
        "DMInstall.bin 解压后未找到 dmdbms 目录（解压路径: {}）",
        tmp.path().display()
    );

    let install_dir = Path::new(install_path);
    if let Some(parent) = install_dir.parent() {
        std::fs::create_dir_all(parent).context("创建安装路径父目录失败")?;
    }

    // 同分区 rename 最快；跨分区 fallback 到 cp -r
    if std::fs::rename(&dmdbms_src, install_dir).is_err() {
        std::fs::create_dir_all(install_dir).context("创建安装目录失败")?;
        let status = Command::new("cp")
            .args(["-r", "--"])
            .arg(dmdbms_src.join("."))
            .arg(install_dir)
            .status()
            .context("cp -r dmdbms 到安装路径失败")?;
        anyhow::ensure!(status.success(), "cp -r 返回非零退出码");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_from_bin_fails_on_nonexistent_file() {
        let result = install_from_bin(Path::new("/nonexistent/DMInstall.bin"), "/tmp/dm_test");
        assert!(result.is_err(), "不存在的文件应返回错误");
    }

    #[test]
    fn test_install_from_bin_fails_when_dmdbms_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let zip_path = tmp.path().join("DMInstall.bin");
        // 写一个不含 dmdbms 的 ZIP
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.add_directory("other/", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.finish().unwrap();

        let install_dir = tmp.path().join("install");
        let result = install_from_bin(&zip_path, install_dir.to_str().unwrap());
        assert!(result.is_err(), "缺少 dmdbms 目录应返回错误");
    }
}
