use anyhow::Result;
use std::path::PathBuf;

/// 从达梦官方渠道下载安装包（Phase 1 占位）。
///
/// Phase 1 主路径使用 --package 本地 ISO，不调用此函数。
/// 待 spike 验证达梦官网直链可行性后填入真实实现。
pub async fn fetch_dm_installer() -> Result<PathBuf> {
    Err(anyhow::anyhow!(
        "自动下载未实现（Phase 1 占位）。\n\
         请使用 --package /path/to/dm.iso 指定本地安装包。"
    ))
}

// Phase 2 实现参考模式（来自 RESEARCH.md）：
//
// use reqwest::Client;
// use indicatif::{ProgressBar, ProgressStyle};
// use tokio::io::AsyncWriteExt;
//
// pub async fn fetch_dm_installer() -> Result<PathBuf> {
//     let url = "TODO: spike 验证达梦官网直链";
//     let client = Client::new();
//     let resp = client.get(url).send().await?;
//     let total = resp.content_length().unwrap_or(0);
//     let pb = ProgressBar::new(total);
//     // ...stream chunks, write to tempfile, pb.inc(bytes)...
// }
