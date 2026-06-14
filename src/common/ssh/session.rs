use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::{client, ChannelMsg};
use russh_sftp::client::SftpSession;
use tokio::io::AsyncWriteExt;

use crate::config::ssh::SshCredentials;

use super::error::SshError;
use super::runner::CommandRunner;

/// TOFU（首次使用信任）主机密钥处理器 —— 无条件接受服务器密钥。
pub struct TofuHandler {
    pub accepted_keys: std::sync::Mutex<Vec<russh::keys::PublicKey>>,
}

impl client::Handler for TofuHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, russh::Error> {
        let fingerprint = server_public_key.fingerprint(Default::default());
        tracing::warn!(
            "[ssh][TOFU] 接受服务器公钥（未验证）: {} — 生产环境请配置 host_key_fingerprint",
            fingerprint
        );
        match self.accepted_keys.lock() {
            Ok(mut keys) => keys.push(server_public_key.clone()),
            Err(poisoned) => poisoned.into_inner().push(server_public_key.clone()),
        }
        Ok(true)
    }
}

/// 基于 russh 的真实 SSH 会话实现。
pub struct SshSession {
    handle: client::Handle<TofuHandler>,
}

impl SshSession {
    /// 建立 SSH 连接，优先使用私钥，其次密码。`user` 从 `creds.user` 读取。
    pub async fn connect(host: &str, port: u16, creds: &SshCredentials) -> Result<Self, SshError> {
        tracing::debug!("[ssh] 正在连接 {}@{}:{}", creds.user, host, port);
        let config = Arc::new(client::Config::default());
        let handler = TofuHandler { accepted_keys: std::sync::Mutex::new(Vec::new()) };
        let addr = format!("{}:{}", host, port);
        let mut handle = client::connect(config, &addr, handler)
            .await
            .map_err(|source| SshError::Connect { host: host.to_string(), source })?;
        try_auth(&mut handle, creds)
            .await
            .map_err(|source| SshError::Connect { host: host.to_string(), source })?;
        tracing::debug!("[ssh] 认证成功: {}@{}:{}", creds.user, host, port);
        Ok(Self { handle })
    }
}

#[async_trait]
impl CommandRunner for SshSession {
    async fn exec(&self, command: &str) -> Result<(Vec<u8>, u32), SshError> {
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| SshError::Connect { host: "session".to_string(), source: e })?;
        channel
            .exec(true, command)
            .await
            .map_err(|e| SshError::Connect { host: "exec".to_string(), source: e })?;
        collect_exec_output(&mut channel, command).await
    }

    async fn sftp_write(&self, remote_path: &str, bytes: &[u8]) -> Result<(), SshError> {
        self.sftp_write_with_progress(remote_path, bytes, &|_| {}).await
    }

    async fn sftp_write_with_progress(
        &self,
        remote_path: &str,
        bytes: &[u8],
        on_chunk: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<(), SshError> {
        const CHUNK: usize = 64 * 1024;
        tracing::debug!("[sftp] 上传 {} bytes -> {}", bytes.len(), remote_path);
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| SshError::Connect { host: "sftp".to_string(), source: e })?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| SshError::Connect { host: "sftp-subsystem".to_string(), source: e })?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|source| SshError::SftpUpload { remote_path: remote_path.to_string(), source })?;
        let mut remote_file = sftp
            .create(remote_path)
            .await
            .map_err(|source| SshError::SftpUpload { remote_path: remote_path.to_string(), source })?;
        for chunk in bytes.chunks(CHUNK) {
            remote_file.write_all(chunk).await.map_err(|io_err| SshError::SftpUpload {
                remote_path: remote_path.to_string(),
                source: russh_sftp::client::error::Error::UnexpectedBehavior(io_err.to_string()),
            })?;
            on_chunk(chunk.len() as u64);
        }
        tracing::debug!("[sftp] 上传完成: {}", remote_path);
        Ok(())
    }
}

/// 尝试密钥或密码鉴权，任一成功即返回。
async fn try_auth(
    handle: &mut client::Handle<TofuHandler>,
    creds: &SshCredentials,
) -> Result<(), russh::Error> {
    if let Some(identity_file) = &creds.identity_file {
        tracing::debug!("[ssh] 尝试私钥认证: {}", identity_file.display());
        if try_key_auth(handle, &creds.user, identity_file).await.is_ok() {
            tracing::debug!("[ssh] 私钥认证成功");
            return Ok(());
        }
        tracing::debug!("[ssh] 私钥认证失败，回退到密码认证");
    }
    if let Some(_password) = &creds.password {
        tracing::debug!("[ssh] 尝试密码认证");
        let result = handle.authenticate_password(&creds.user, _password.clone()).await?;
        if result.success() {
            tracing::debug!("[ssh] 密码认证成功");
            return Ok(());
        }
    }
    Err(russh::Error::NotAuthenticated)
}

/// 尝试公钥鉴权。
async fn try_key_auth(
    handle: &mut client::Handle<TofuHandler>,
    user: &str,
    identity_file: &Path,
) -> Result<(), russh::Error> {
    let expanded = expand_tilde(identity_file);
    let key_pair = load_secret_key(&expanded, None)?;
    let rsa_hash = handle.best_supported_rsa_hash().await?.flatten();
    let key = PrivateKeyWithHashAlg::new(Arc::new(key_pair), rsa_hash);
    let result = handle.authenticate_publickey(user, key).await?;
    if result.success() { Ok(()) } else { Err(russh::Error::NotAuthenticated) }
}

/// 从 SSH channel 收集命令输出和退出码。
async fn collect_exec_output(
    channel: &mut russh::Channel<client::Msg>,
    command: &str,
) -> Result<(Vec<u8>, u32), SshError> {
    tracing::debug!("[ssh] 执行远端命令: {}", command);
    let mut stdout = Vec::new();
    let mut exit_code = 0u32;
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { ref data }) => stdout.extend_from_slice(data),
            Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = exit_status,
            Some(ChannelMsg::Eof) | None => break,
            _ => {}
        }
    }
    tracing::debug!(
        "[ssh] 命令完成: exit_code={}, stdout={} bytes",
        exit_code,
        stdout.len()
    );
    if exit_code != 0 {
        let output = String::from_utf8_lossy(&stdout).trim().to_string();
        tracing::warn!("[ssh] 命令失败 (exit {}): {}", exit_code, output);
        return Err(SshError::ExecFailed { command: command.to_string(), exit_code, output });
    }
    Ok((stdout, exit_code))
}

/// Rust PathBuf 不会自动展开 `~`，需要手动处理。
/// Windows 用 USERPROFILE，Unix 用 HOME；两种路径分隔符均支持。
fn expand_tilde(path: &Path) -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    expand_tilde_with_home(path, home.as_deref())
}

fn expand_tilde_with_home(path: &Path, home: Option<&std::ffi::OsStr>) -> PathBuf {
    let s = match path.to_str() {
        Some(s) => s,
        None => return path.to_path_buf(),
    };
    let rest = if let Some(r) = s.strip_prefix("~/") {
        r
    } else if let Some(r) = s.strip_prefix("~\\") {
        r
    } else {
        return path.to_path_buf();
    };
    match home {
        Some(h) => PathBuf::from(h).join(rest),
        None => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use russh::client::Handler;
    use super::*;

    fn home(s: &str) -> Option<&std::ffi::OsStr> {
        Some(std::ffi::OsStr::new(s))
    }

    #[test]
    fn test_expand_tilde_replaces_home() {
        let expanded = expand_tilde_with_home(&PathBuf::from("~/.ssh/id_rsa"), home("/home/testuser"));
        assert_eq!(expanded, PathBuf::from("/home/testuser/.ssh/id_rsa"));
    }

    #[test]
    fn test_expand_tilde_no_tilde_unchanged() {
        let path = PathBuf::from("/absolute/path/key");
        assert_eq!(expand_tilde_with_home(&path, home("/home/x")), path);
    }

    #[test]
    fn test_expand_tilde_missing_home_returns_input() {
        let path = PathBuf::from("~/foo");
        assert_eq!(expand_tilde_with_home(&path, None), path);
    }

    #[test]
    #[cfg(windows)]
    fn test_expand_tilde_backslash_windows() {
        let expanded = expand_tilde_with_home(
            &PathBuf::from("~\\.ssh\\id_rsa"),
            home("C:\\Users\\testuser"),
        );
        assert_eq!(expanded, PathBuf::from("C:\\Users\\testuser\\.ssh\\id_rsa"));
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_tofu_logs_fingerprint() {
        const TEST_PUBKEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti test@example";
        let public_key = russh::keys::PublicKey::from_openssh(TEST_PUBKEY).expect("解析公钥");
        let mut handler = TofuHandler { accepted_keys: std::sync::Mutex::new(Vec::new()) };
        handler.check_server_key(&public_key).await.unwrap();
        assert!(logs_contain("[ssh][TOFU]"));
    }
}
