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
    pub fn from_user_path(path: PathBuf) -> Self {
        Self { path, _owned_dir: None }
    }

    fn from_download(path: PathBuf, dir: TempDir) -> Self {
        Self { path, _owned_dir: Some(dir) }
    }
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

    if matches.is_empty() {
        if let Some(os_str) = &platform.os {
            // 第一步：前缀匹配 os_str 本身（处理 "kylin10" → "kylin10_sp1" 的向上匹配）
            let prefix_matches = versions::filter_entries_os_prefix(
                &all, &platform.arch, platform.cpu.as_deref(), os_str,
            );
            if !prefix_matches.is_empty() {
                tracing::warn!("OS '{}' 无精确匹配，自动选用 '{}'", os_str, prefix_matches[0].os);
                matches = prefix_matches;
            } else if let Some(base) = os_str.split("_sp").next() {
                // 第二步：去掉 _sp 后缀后精确匹配基础版本（"kylin10_sp1" → "kylin10"）
                // 用 exact match 而非 prefix，防止误命中更高 SP 版本（如 kylin10_sp3）
                if base != os_str {
                    let base_matches = versions::filter_entries(
                        &all, &platform.arch, platform.cpu.as_deref(), Some(base),
                    );
                    if !base_matches.is_empty() {
                        tracing::warn!("OS '{}' 无精确匹配，回退到基础版本 '{}'", os_str, base);
                        matches = base_matches;
                    }
                }
            }
        }
    }

    let entry = select::select_version(&all, &matches, &platform.arch)?;

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

