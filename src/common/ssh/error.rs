use thiserror::Error;

#[derive(Debug, Error)]
pub enum SshError {
    #[error("SSH 连接失败 {host}: {source}")]
    Connect {
        host: String,
        #[source]
        source: russh::Error,
    },
    #[error("SSH 命令执行失败 (exit {exit_code}): {command}\n远端输出:\n{output}")]
    ExecFailed { command: String, exit_code: u32, output: String },
    #[error("SFTP 上传失败 {remote_path}: {source}")]
    SftpUpload {
        remote_path: String,
        #[source]
        source: russh_sftp::client::error::Error,
    },
    #[error("SFTP 下载失败 {remote_path}: {source}")]
    SftpDownload {
        remote_path: String,
        #[source]
        source: russh_sftp::client::error::Error,
    },
}
