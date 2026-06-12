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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_stub_returns_error() {
        // Phase 1 占位应返回含引导信息的错误
        let result = fetch_dm_installer().await;
        assert!(result.is_err(), "占位函数应返回 Err");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("Phase 1 占位"),
            "错误消息应含 'Phase 1 占位'，实际: {msg}"
        );
        assert!(
            msg.contains("--package"),
            "错误消息应含 '--package'，实际: {msg}"
        );
    }
}
