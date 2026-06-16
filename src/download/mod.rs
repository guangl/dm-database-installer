mod http;
mod select;
pub mod versions;

use anyhow::{Context, Result};
use std::path::PathBuf;
use tempfile::TempDir;

use crate::platform::{Platform, detect_platform};

/// 持有安装包路径，并可选地保持下载临时目录的生命周期。
pub struct PackageHandle {
    pub path: PathBuf,
    _owned_dir: Option<TempDir>,
}

impl PackageHandle {
    fn from_download(path: PathBuf, dir: TempDir) -> Self {
        Self {
            path,
            _owned_dir: Some(dir),
        }
    }
}

/// 从指定 URL 下载安装包（支持 .zip 自动解压，.iso/.bin 直接使用）。
///
/// `sha256` 非 `None` 时在解压前校验文件完整性。
pub async fn fetch_from_url(url: &str, sha256: Option<&str>) -> Result<PackageHandle> {
    let file_name = url.split('/').next_back().unwrap_or("dm_installer");
    crate::ui::log_info(&format!("下载安装包: {}", file_name));

    let download_dir = TempDir::new().context("创建临时目录失败")?;
    let dest = download_dir.path().join(file_name);

    http::download_with_progress(url, &dest).await?;
    crate::ui::log_ok("下载完成");

    if let Some(expected) = sha256 {
        crate::ui::log_info("校验 SHA-256...");
        http::verify_sha256(&dest, expected)?;
        crate::ui::log_ok("SHA-256 校验通过");
    }

    let installer = if file_name.to_lowercase().ends_with(".zip") {
        crate::ui::log_info("解压安装包...");
        let extracted = http::extract_zip_installer(&dest, download_dir.path())?;
        crate::ui::log_ok("解压完成");
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
    crate::ui::log_info(&format!(
        "平台检测: arch={}  cpu={}  os={}",
        platform.arch,
        platform.cpu.as_deref().unwrap_or("unknown"),
        platform.os.as_deref().unwrap_or("unknown"),
    ));

    let all = versions::parse_versions();
    let mut matches = versions::filter_entries(
        &all,
        &platform.arch,
        platform.cpu.as_deref(),
        platform.os.as_deref(),
    );

    if matches.is_empty()
        && let Some(os_str) = &platform.os
    {
        for prefix in os_fallback_prefixes(os_str) {
            let prefix_matches = versions::filter_entries_os_prefix(
                &all,
                &platform.arch,
                platform.cpu.as_deref(),
                prefix,
            );
            if !prefix_matches.is_empty() {
                crate::ui::log_warn(&format!(
                    "OS '{}' 无精确匹配，自动选用最近版本 '{}'",
                    os_str, prefix_matches[0].os
                ));
                matches = prefix_matches;
                break;
            }
        }
    }

    let entry = select::select_version(&all, &matches, &platform.arch)?;
    crate::ui::log_ok(&format!("匹配安装包: {}", entry.file_name()));
    fetch_from_url(&entry.url, entry.sha256.as_deref()).await
}

/// 构建 OS 前缀回退链：先精确前缀，再去掉 _sp* 后缀降级。
/// 例：kylin10_sp1 → ["kylin10_sp1", "kylin10"]；kylin10 → ["kylin10"]
fn os_fallback_prefixes(os: &str) -> Vec<&str> {
    let mut prefixes = vec![os];
    if let Some(base) = os.split("_sp").next()
        && base != os
    {
        prefixes.push(base);
    }
    prefixes
}
