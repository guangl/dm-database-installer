use anyhow::Result;
use async_trait::async_trait;

use super::error::SshError;

/// SSH 命令执行与文件上传能力抽象，支持真实 SSH 和测试 mock 注入。
#[async_trait]
pub trait CommandRunner: Send + Sync {
    /// 执行远端命令，返回 (stdout_bytes, exit_code)。
    async fn exec(&self, command: &str) -> Result<(Vec<u8>, u32), SshError>;
    /// 将字节内容上传到远端路径。
    async fn sftp_write(&self, remote_path: &str, bytes: &[u8]) -> Result<(), SshError>;
    /// 分块上传，每写入一块后调用 on_chunk(已传输字节数)。
    /// 默认实现一次性写入并在完成时调用一次回调（供 mock 使用）。
    async fn sftp_write_with_progress(
        &self,
        remote_path: &str,
        bytes: &[u8],
        on_chunk: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<(), SshError> {
        self.sftp_write(remote_path, bytes).await?;
        on_chunk(bytes.len() as u64);
        Ok(())
    }
    /// 通过 SFTP setstat 设置远端文件权限（Unix mode，如 0o755）。
    /// 默认 no-op，供 mock 和不需要权限控制的实现使用。
    async fn sftp_set_permissions(&self, _remote_path: &str, _mode: u32) -> Result<(), SshError> {
        Ok(())
    }
}
