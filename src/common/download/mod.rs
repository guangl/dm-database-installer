
pub mod versions;
mod http;

use anyhow::{bail, Context, Result};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use tempfile::TempDir;

use crate::common::download::versions::VersionEntry;

/// 持有安装包路径，并可选地保持下载临时目录的生命周期。
pub struct PackageHandle {
    pub path: PathBuf,
    _owned_dir: Option<TempDir>,
}

impl PackageHandle {
    pub fn from_user_path(path: PathBuf) -> Self {
        PackageHandle { path, _owned_dir: None }
    }

    fn from_download(path: PathBuf, dir: TempDir) -> Self {
        PackageHandle { path, _owned_dir: Some(dir) }
    }
}

/// 根据 versions.txt 自动检测平台、选择版本并下载安装包。
///
/// `non_interactive=true` 时多个候选中自动选第一项（`--defaults` / `--yes` 模式）。
pub async fn fetch_dm_installer(non_interactive: bool) -> Result<PackageHandle> {
    let platform = crate::common::sysinfo::detect_platform();
    tracing::debug!(
        "检测平台: arch={}, cpu={:?}, os={:?}",
        platform.arch, platform.cpu, platform.os
    );

    let all = versions::parse_versions();
    let matches = versions::filter_entries(
        &all,
        &platform.arch,
        platform.cpu.as_deref(),
        platform.os.as_deref(),
    );

    let entry = select_version(&all, &matches, &platform.arch, non_interactive)?;
    download_entry(entry).await
}

async fn download_entry(entry: &VersionEntry) -> Result<PackageHandle> {
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

fn select_version<'a>(
    all: &'a [VersionEntry],
    matches: &[&'a VersionEntry],
    arch: &str,
    non_interactive: bool,
) -> Result<&'a VersionEntry> {
    if matches.is_empty() {
        return bail_no_match(all, arch);
    }
    if matches.len() == 1 {
        let e = matches[0];
        println!("匹配版本: {} ({} {})", e.file_name(), e.cpu, e.os);
        return Ok(e);
    }
    if non_interactive {
        let e = matches[0];
        println!("多个匹配，自动选择: {} ({} {})", e.file_name(), e.cpu, e.os);
        return Ok(e);
    }
    prompt_selection(matches)
}

fn bail_no_match<'a>(all: &[VersionEntry], arch: &str) -> Result<&'a VersionEntry> {
    let for_arch: Vec<String> = all
        .iter()
        .filter(|e| e.arch == arch)
        .map(|e| format!("  {} {} - {}", e.cpu, e.os, e.file_name()))
        .collect();

    if for_arch.is_empty() {
        let arches: Vec<&str> = {
            let mut seen = std::collections::HashSet::new();
            all.iter().filter(|e| seen.insert(e.arch.as_str())).map(|e| e.arch.as_str()).collect()
        };
        bail!("不支持当前架构 {}。支持的架构: {}", arch, arches.join(", "));
    }
    bail!(
        "无法自动匹配当前系统 OS。架构 {} 的可用版本:\n{}",
        arch,
        for_arch.join("\n")
    );
}

fn prompt_selection<'a>(matches: &[&'a VersionEntry]) -> Result<&'a VersionEntry> {
    println!("检测到多个匹配版本，请选择：");
    for (i, e) in matches.iter().enumerate() {
        println!("  [{}] {} ({} {})", i + 1, e.file_name(), e.cpu, e.os);
    }
    print!("请输入编号 [1-{}]: ", matches.len());
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input)?;
    let n: usize = input
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("无效输入: {}", input.trim()))?;

    if n == 0 || n > matches.len() {
        bail!("编号 {} 超出范围 [1-{}]", n, matches.len());
    }
    Ok(matches[n - 1])
}
