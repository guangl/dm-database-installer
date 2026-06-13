use anyhow::{bail, Context, Result};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// 带进度条的 HTTPS 下载，写入 `dest`。
pub async fn download_with_progress(url: &str, dest: &Path) -> Result<()> {
    tracing::debug!("开始下载: {} -> {}", url, dest.display());
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("HTTP 请求失败: {}", url))?;

    if !response.status().is_success() {
        bail!("HTTP 错误 {}: {}", response.status(), url);
    }

    let content_length = response.content_length();
    tracing::debug!(
        "HTTP 响应: status={}, content_length={:?}",
        response.status(),
        content_length
    );

    let pb = build_progress_bar(content_length);
    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("创建文件失败: {}", dest.display()))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("下载中断")?;
        file.write_all(&chunk).await.context("写入文件失败")?;
        pb.inc(chunk.len() as u64);
    }
    file.flush().await.context("刷新文件失败")?;
    pb.finish_with_message("下载完成");
    tracing::debug!("下载完成: {} bytes 写入 {}", pb.position(), dest.display());
    Ok(())
}

fn build_progress_bar(total: Option<u64>) -> ProgressBar {
    match total {
        Some(size) => {
            let pb = ProgressBar::new(size);
            pb.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} @ {bytes_per_sec}, ETA {eta}",
                )
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
            );
            pb
        }
        None => {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::with_template("{spinner:.green} 已下载 {bytes} @ {bytes_per_sec}")
                    .unwrap_or_else(|_| ProgressStyle::default_spinner()),
            );
            pb
        }
    }
}

/// 从 zip 中提取 `.iso`（优先）或 `DMInstall.bin`，写入 `extract_dir`，返回其路径。
pub fn extract_zip_installer(zip_path: &Path, extract_dir: &Path) -> Result<PathBuf> {
    tracing::debug!("解压 zip: {}", zip_path.display());
    let file = std::fs::File::open(zip_path)
        .with_context(|| format!("无法打开 zip: {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("zip 格式无效")?;
    tracing::debug!("zip 共 {} 个条目", archive.len());

    let target = find_installer_entry(&mut archive)?;
    tracing::debug!("选取安装包条目: {}", target);
    let dest = extract_entry(&mut archive, &target, extract_dir)?;
    tracing::debug!("已解压到: {}", dest.display());
    Ok(dest)
}

fn find_installer_entry(archive: &mut zip::ZipArchive<std::fs::File>) -> Result<String> {
    let mut iso_name: Option<String> = None;
    let mut bin_name: Option<String> = None;
    for i in 0..archive.len() {
        let name = archive
            .by_index(i)
            .with_context(|| format!("读取 zip 条目 {} 失败", i))?
            .name()
            .to_string();
        if name.to_lowercase().ends_with(".iso") {
            iso_name = Some(name);
        } else if name.ends_with("DMInstall.bin") {
            bin_name = Some(name);
        }
    }
    iso_name.or(bin_name).ok_or_else(|| {
        anyhow::anyhow!("zip 中未找到 .iso 或 DMInstall.bin")
    })
}

fn extract_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    entry_name: &str,
    dest_dir: &Path,
) -> Result<PathBuf> {
    let mut entry = archive
        .by_name(entry_name)
        .with_context(|| format!("zip 中找不到: {}", entry_name))?;

    let file_name = Path::new(entry_name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| entry_name.to_string());

    let dest = dest_dir.join(&file_name);
    let mut out =
        std::fs::File::create(&dest).with_context(|| format!("创建文件失败: {}", dest.display()))?;
    std::io::copy(&mut entry, &mut out).context("解压文件失败")?;
    Ok(dest)
}
