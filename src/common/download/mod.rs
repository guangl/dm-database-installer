mod http;
mod select;
pub mod versions;

use anyhow::{Context, Result};
use std::path::PathBuf;
use tempfile::TempDir;

use crate::common::sysinfo::detect_platform;

/// 持有安装包路径，并可选地保持下载临时目录的生命周期。
pub struct PackageHandle {
    pub path: PathBuf,
    _owned_dir: Option<TempDir>,
}

impl PackageHandle {
    pub fn from_user_path(path: PathBuf) -> Self {
        Self { path, _owned_dir: None }
    }

    fn from_download(path: PathBuf, dir: TempDir) -> Self {
        Self { path, _owned_dir: Some(dir) }
    }
}

/// 根据 versions.txt 自动检测平台、选择版本并下载安装包。
///
/// `non_interactive=true` 时多个候选中自动选第一项（`--defaults` / `--yes` 模式）。
pub async fn fetch_dm_installer(non_interactive: bool) -> Result<PackageHandle> {
    let platform = detect_platform();
    tracing::debug!("检测平台: arch={}, cpu={:?}, os={:?}", platform.arch, platform.cpu, platform.os);

    let all = versions::parse_versions();
    let matches = versions::filter_entries(&all, &platform.arch, platform.cpu.as_deref(), platform.os.as_deref());
    let entry = select::select_version(&all, &matches, &platform.arch, non_interactive)?;

    let file_name = entry.file_name();
    println!("下载安装包: {}", file_name);
    println!("来源: {}", entry.url);

    let download_dir = TempDir::new().context("创建临时目录失败")?;
    let zip_path = download_dir.path().join(file_name);

    http::download_with_progress(&entry.url, &zip_path).await?;

    println!("解压安装包...");
    let installer = http::extract_zip_installer(&zip_path, download_dir.path())?;
    println!("已解压: {}", installer.display());

    Ok(PackageHandle::from_download(installer, download_dir))
}
