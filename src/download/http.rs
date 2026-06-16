use anyhow::{Context, Result, bail};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// 带进度条的 HTTPS 下载，写入 `dest`。
pub async fn download_with_progress(url: &str, dest: &Path) -> Result<()> {
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
    pb.finish_and_clear();
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

/// 计算文件 SHA-256 并与期望值对比；不匹配时返回错误。
pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("无法打开: {}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).context("读取文件失败")?;
    let actual = hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();
    if actual != expected.to_lowercase() {
        bail!("SHA-256 校验失败\n  期望: {}\n  实际: {}", expected, actual);
    }
    Ok(())
}

/// 从 zip 中提取 `.iso`（优先）或 `DMInstall.bin`，写入 `extract_dir`，返回其路径。
/// 若 zip 内含 sha256 校验文件，自动校验提取结果。
pub fn extract_zip_installer(zip_path: &Path, extract_dir: &Path) -> Result<PathBuf> {
    let file = std::fs::File::open(zip_path)
        .with_context(|| format!("无法打开 zip: {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("zip 格式无效")?;

    let contents = scan_zip_entries(&mut archive)?;
    let dest = extract_entry(&mut archive, &contents.installer_name, extract_dir)?;

    if let Some(sha256_entry) = contents.sha256_name {
        let raw = read_zip_text(&mut archive, &sha256_entry)?;
        let file_name = Path::new(&contents.installer_name)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| contents.installer_name.clone());
        if let Some(expected) = parse_sha256_for(&raw, &file_name) {
            crate::ui::log_info("校验 zip 内嵌 SHA-256...");
            verify_sha256(&dest, &expected)?;
            crate::ui::log_ok("zip 内嵌 SHA-256 校验通过");
        }
    }

    Ok(dest)
}

struct ZipContents {
    installer_name: String,
    sha256_name: Option<String>,
}

fn scan_zip_entries(archive: &mut zip::ZipArchive<std::fs::File>) -> Result<ZipContents> {
    let mut iso_name: Option<String> = None;
    let mut bin_name: Option<String> = None;
    let mut sha256_name: Option<String> = None;
    for i in 0..archive.len() {
        let name = archive
            .by_index(i)
            .with_context(|| format!("读取 zip 条目 {} 失败", i))?
            .name()
            .to_string();
        let lower = name.to_lowercase();
        if lower.ends_with(".iso") {
            iso_name = Some(name);
        } else if name.ends_with("DMInstall.bin") {
            bin_name = Some(name);
        } else if lower.ends_with("_sha256.txt") {
            sha256_name = Some(name);
        }
    }
    let installer_name = iso_name
        .or(bin_name)
        .ok_or_else(|| anyhow::anyhow!("zip 中未找到 .iso 或 DMInstall.bin"))?;
    Ok(ZipContents {
        installer_name,
        sha256_name,
    })
}

/// 从 zip 中读取文本条目内容。
fn read_zip_text(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<String> {
    use std::io::Read;
    let mut entry = archive
        .by_name(name)
        .with_context(|| format!("zip 中找不到: {}", name))?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).context("读取 sha256 文件失败")?;
    Ok(buf)
}

/// 从 `文件名_SHA256.txt` 第二行提取 SHA-256 十六进制串（64位）。
fn parse_sha256_for(content: &str, _file_name: &str) -> Option<String> {
    let line = content.lines().nth(1)?.trim();
    if line.len() == 64 && line.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(line.to_lowercase())
    } else {
        None
    }
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
    let mut out = std::fs::File::create(&dest)
        .with_context(|| format!("创建文件失败: {}", dest.display()))?;
    std::io::copy(&mut entry, &mut out).context("解压文件失败")?;
    Ok(dest)
}
