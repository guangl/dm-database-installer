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
}
