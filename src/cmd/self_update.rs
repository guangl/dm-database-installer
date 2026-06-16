use anyhow::{Context, Result, bail};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::Deserialize;
use std::io::Read;

const REPO: &str = "guangl/dm-database-installer";

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn run(check_only: bool) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");

    println!("当前版本: v{current_version}");
    println!("正在检查最新版本...");

    let client = Client::builder()
        .user_agent(format!("dm-installer/{current_version}"))
        .build()?;

    let release = fetch_latest_release(&client).await?;
    let latest_tag = &release.tag_name;
    let latest_version = latest_tag.trim_start_matches('v');

    println!("最新版本: {latest_tag}");

    if latest_version == current_version {
        println!("已是最新版本，无需更新。");
        return Ok(());
    }

    println!("发现新版本: v{current_version} → {latest_tag}");

    if check_only {
        println!("运行 `dm-installer self-update` 以安装更新。");
        return Ok(());
    }

    let target = detect_target()?;
    let asset = find_asset(&release, target)?;

    println!("下载 {}...", asset.name);

    let bytes = download_with_progress(&client, asset).await?;
    let binary = extract_binary(&bytes, &asset.name)?;

    let exe_path = std::env::current_exe().context("无法获取当前可执行文件路径")?;
    replace_binary(&exe_path, &binary)?;

    println!("更新完成！已升级到 {latest_tag}。");

    Ok(())
}

async fn fetch_latest_release(client: &Client) -> Result<GithubRelease> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .context("请求 GitHub API 失败")?
        .error_for_status()
        .context("GitHub API 返回错误状态")?
        .json::<GithubRelease>()
        .await
        .context("解析 GitHub Release 响应失败")
}

fn detect_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        (os, arch) => bail!("不支持的平台: {os}/{arch}"),
    }
}

fn find_asset<'a>(release: &'a GithubRelease, target: &str) -> Result<&'a GithubAsset> {
    release
        .assets
        .iter()
        .find(|a| a.name.contains(target))
        .ok_or_else(|| anyhow::anyhow!("找不到当前平台 ({target}) 的发布资产"))
}

async fn download_with_progress(client: &Client, asset: &GithubAsset) -> Result<Vec<u8>> {
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await?
        .error_for_status()?;

    let total = response.content_length().unwrap_or(0);
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template("{bar:40} {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("=>-"),
    );

    let mut bytes = Vec::with_capacity(total as usize);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        pb.inc(chunk.len() as u64);
        bytes.extend_from_slice(&chunk);
    }
    pb.finish_and_clear();

    Ok(bytes)
}

fn extract_binary(bytes: &[u8], asset_name: &str) -> Result<Vec<u8>> {
    if asset_name.ends_with(".tar.gz") {
        extract_from_tar_gz(bytes)
    } else if asset_name.ends_with(".zip") {
        extract_from_zip(bytes)
    } else {
        bail!("不支持的压缩格式: {asset_name}")
    }
}

fn extract_from_tar_gz(bytes: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let decoder = GzDecoder::new(bytes);
    let mut archive = Archive::new(decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        if file_name == "dm-installer" || file_name == "dm-installer.exe" {
            let mut binary = Vec::new();
            entry.read_to_end(&mut binary)?;
            return Ok(binary);
        }
    }
    bail!("压缩包中找不到 dm-installer 可执行文件")
}

fn extract_from_zip(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::Cursor;
    use zip::ZipArchive;

    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if name == "dm-installer.exe" || name == "dm-installer" {
            let mut binary = Vec::new();
            file.read_to_end(&mut binary)?;
            return Ok(binary);
        }
    }
    bail!("ZIP 包中找不到 dm-installer 可执行文件")
}

fn replace_binary(exe_path: &std::path::Path, binary: &[u8]) -> Result<()> {
    use std::io::Write;

    let parent = exe_path.parent().context("无法获取可执行文件所在目录")?;
    let temp_path = parent.join(format!(".dm-installer.tmp.{}", std::process::id()));

    let mut temp_file = std::fs::File::create(&temp_path).context("创建临时文件失败")?;
    temp_file.write_all(binary)?;
    temp_file.flush()?;
    drop(temp_file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&temp_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&temp_path, perms)?;
    }

    std::fs::rename(&temp_path, exe_path).context("替换可执行文件失败")?;

    Ok(())
}
