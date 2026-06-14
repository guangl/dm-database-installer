mod http;
mod select;
pub mod versions;

use anyhow::{Context, Result};
use std::path::PathBuf;
use tempfile::TempDir;

use crate::common::sysinfo::{detect_platform, Platform};

/// 持有安装包路径，并可选地保持下载临时目录的生命周期。
pub struct PackageHandle {
    pub path: PathBuf,
    _owned_dir: Option<TempDir>,
}

impl PackageHandle {
    pub fn from_path(path: PathBuf) -> Self {
        Self { path, _owned_dir: None }
    }
    fn from_download(path: PathBuf, dir: TempDir) -> Self {
        Self { path, _owned_dir: Some(dir) }
    }
}

/// 从指定 URL 下载安装包（支持 .zip 自动解压，.iso/.bin 直接使用）。
pub async fn fetch_from_url(url: &str) -> Result<PackageHandle> {
    let file_name = url.split('/').next_back().unwrap_or("dm_installer");
    println!("下载安装包: {}", file_name);
    println!("来源: {}", url);

    let download_dir = TempDir::new().context("创建临时目录失败")?;
    let dest = download_dir.path().join(file_name);

    http::download_with_progress(url, &dest).await?;

    let installer = if file_name.to_lowercase().ends_with(".zip") {
        println!("解压安装包...");
        let extracted = http::extract_zip_installer(&dest, download_dir.path())?;
        println!("已解压: {}", extracted.display());
        extracted
    } else {
        dest
    };

    Ok(PackageHandle::from_download(installer, download_dir))
}

/// 根据 versions.txt 自动检测本地平台并下载安装包。
pub async fn fetch_dm_installer() -> Result<PackageHandle> {
    let platform = detect_platform();
    fetch_dm_installer_for(&platform).await
}

/// 根据指定平台从 versions.txt 选择并下载安装包。
pub async fn fetch_dm_installer_for(platform: &Platform) -> Result<PackageHandle> {
    tracing::debug!("目标平台: arch={}, cpu={:?}, os={:?}", platform.arch, platform.cpu, platform.os);

    let all = versions::parse_versions();
    let mut matches = versions::filter_entries(&all, &platform.arch, platform.cpu.as_deref(), platform.os.as_deref());

    if matches.is_empty() && let Some(os_str) = &platform.os {
        for prefix in os_fallback_prefixes(os_str) {
            let prefix_matches = versions::filter_entries_os_prefix(&all, &platform.arch, platform.cpu.as_deref(), prefix);
            if !prefix_matches.is_empty() {
                tracing::warn!("OS '{}' 无精确匹配，自动选用最近版本 '{}'", os_str, prefix_matches[0].os);
                matches = prefix_matches;
                break;
            }
        }
    }

    let entry = select::select_version(&all, &matches, &platform.arch)?;
    fetch_from_url(&entry.url).await
}

/// 构建 OS 前缀回退链：先精确前缀，再去掉 _sp* 后缀降级。
/// 例：kylin10_sp1 → ["kylin10_sp1", "kylin10"]；kylin10 → ["kylin10"]
fn os_fallback_prefixes(os: &str) -> Vec<&str> {
    let mut prefixes = vec![os];
    if let Some(base) = os.split("_sp").next() && base != os {
        prefixes.push(base);
    }
    prefixes
}
