use async_trait::async_trait;

use super::error::SshError;
use super::runner::CommandRunner;

/// 本地命令执行器，实现 CommandRunner trait。
/// exec 通过 sh -c 执行命令，非零退出码返回 SshError::ExecFailed（与 SSH 行为一致）。
/// sftp_write/sftp_read 对应本地文件系统读写。
pub struct LocalRunner;

#[async_trait]
impl CommandRunner for LocalRunner {
    async fn exec(&self, command: &str) -> Result<(Vec<u8>, u32), SshError> {
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .await
            .map_err(|e| SshError::ExecFailed {
                command: command.to_string(),
                exit_code: 127,
                output: e.to_string(),
            })?;

        let exit_code = output.status.code().unwrap_or(1) as u32;
        if exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            return Err(SshError::ExecFailed {
                command: command.to_string(),
                exit_code,
                output: detail,
            });
        }
        Ok((output.stdout, exit_code))
    }

    async fn sftp_write(&self, remote_path: &str, bytes: &[u8]) -> Result<(), SshError> {
        tokio::fs::write(remote_path, bytes).await.map_err(|e| SshError::Io {
            path: remote_path.to_string(),
            source: e,
        })
    }

    async fn sftp_read(&self, remote_path: &str) -> Result<Vec<u8>, SshError> {
        tokio::fs::read(remote_path).await.map_err(|e| SshError::Io {
            path: remote_path.to_string(),
            source: e,
        })
    }
}
