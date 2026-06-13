use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::{client, ChannelMsg};
use russh_sftp::client::SftpSession;
use thiserror::Error;
use tokio::io::AsyncWriteExt;

use crate::config::cluster::SshCredentials;

/// SSH 操作错误类型，覆盖连接失败、命令执行失败、SFTP 上传失败。
#[derive(Debug, Error)]
pub enum SshError {
    #[error("SSH 连接失败 {host}: {source}")]
    Connect {
        host: String,
        #[source]
        source: russh::Error,
    },
    #[error("SSH 命令执行失败 (exit {exit_code}): {command}")]
    ExecFailed { command: String, exit_code: u32 },
    #[error("SFTP 上传失败 {remote_path}: {source}")]
    SftpUpload {
        remote_path: String,
        #[source]
        source: russh_sftp::client::error::Error,
    },
}

/// SSH 命令执行与文件上传能力抽象，支持真实 SSH 和测试 mock 注入。
#[async_trait]
pub trait CommandRunner: Send + Sync {
    /// 执行远端命令，返回 (stdout_bytes, exit_code)。
    async fn exec(&self, command: &str) -> Result<(Vec<u8>, u32), SshError>;
    /// 将字节内容上传到远端路径。
    async fn sftp_write(&self, remote_path: &str, bytes: &[u8]) -> Result<(), SshError>;
}

/// TOFU（首次使用信任）主机密钥处理器 —— 无条件接受服务器密钥（D-07）。
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
            Ok(mut accepted) => accepted.push(server_public_key.clone()),
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
    /// 建立 SSH 连接，优先使用私钥，其次密码。
    pub async fn connect(
        host: &str,
        port: u16,
        user: &str,
        creds: &SshCredentials,
    ) -> Result<Self, SshError> {
        let config = Arc::new(client::Config::default());
        let handler = TofuHandler {
            accepted_keys: std::sync::Mutex::new(Vec::new()),
        };
        let addr = format!("{}:{}", host, port);
        let mut handle = client::connect(config, addr.as_str(), handler)
            .await
            .map_err(|source| SshError::Connect {
                host: host.to_string(),
                source,
            })?;
        try_auth(&mut handle, user, creds)
            .await
            .map_err(|source| SshError::Connect {
                host: host.to_string(),
                source,
            })?;
        Ok(Self { handle })
    }
}

/// 尝试密钥或密码鉴权，任一成功即返回。
async fn try_auth(
    handle: &mut client::Handle<TofuHandler>,
    user: &str,
    creds: &SshCredentials,
) -> Result<(), russh::Error> {
    if let Some(identity_file) = &creds.identity_file {
        if try_key_auth(handle, user, identity_file).await.is_ok() {
            return Ok(());
        }
    }
    if let Some(password) = &creds.password {
        let result = handle
            .authenticate_password(user, password.clone())
            .await?;
        if result.success() {
            return Ok(());
        }
    }
    Err(russh::Error::NotAuthenticated)
}

/// 展开路径中的 `~/` 前缀为 $HOME 环境变量值（CR-03）。
/// Rust PathBuf 不会自动展开 `~`，需要手动处理。
/// 若路径无 `~/` 前缀或 HOME 未设置，则原样返回。
fn expand_tilde(path: &std::path::PathBuf) -> std::path::PathBuf {
    if let Some(path_str) = path.to_str() {
        if let Some(rest) = path_str.strip_prefix("~/") {
            if let Some(home_dir) = std::env::var_os("HOME") {
                return std::path::PathBuf::from(home_dir).join(rest);
            }
        }
    }
    path.clone()
}

/// 尝试公钥鉴权。
async fn try_key_auth(
    handle: &mut client::Handle<TofuHandler>,
    user: &str,
    identity_file: &std::path::PathBuf,
) -> Result<(), russh::Error> {
    let expanded_path = expand_tilde(identity_file);
    let key_pair = load_secret_key(&expanded_path, None)?;
    let rsa_hash = handle.best_supported_rsa_hash().await?.flatten();
    let key = PrivateKeyWithHashAlg::new(Arc::new(key_pair), rsa_hash);
    let result = handle.authenticate_publickey(user, key).await?;
    if result.success() {
        Ok(())
    } else {
        Err(russh::Error::NotAuthenticated)
    }
}

#[async_trait]
impl CommandRunner for SshSession {
    async fn exec(&self, command: &str) -> Result<(Vec<u8>, u32), SshError> {
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| SshError::Connect {
                host: "session".to_string(),
                source: e,
            })?;
        channel.exec(true, command).await.map_err(|e| SshError::Connect {
            host: "exec".to_string(),
            source: e,
        })?;
        collect_exec_output(&mut channel, command).await
    }

    async fn sftp_write(&self, remote_path: &str, bytes: &[u8]) -> Result<(), SshError> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| SshError::Connect {
                host: "sftp".to_string(),
                source: e,
            })?;
        channel.request_subsystem(true, "sftp").await.map_err(|e| SshError::Connect {
            host: "sftp-subsystem".to_string(),
            source: e,
        })?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|source| SshError::SftpUpload {
                remote_path: remote_path.to_string(),
                source,
            })?;
        let mut remote_file = sftp
            .create(remote_path)
            .await
            .map_err(|source| SshError::SftpUpload {
                remote_path: remote_path.to_string(),
                source,
            })?;
        remote_file
            .write_all(bytes)
            .await
            .map_err(|io_err| SshError::SftpUpload {
                remote_path: remote_path.to_string(),
                source: russh_sftp::client::error::Error::UnexpectedBehavior(
                    io_err.to_string(),
                ),
            })
    }
}

/// 从 SSH channel 收集命令输出和退出码。
async fn collect_exec_output(
    channel: &mut russh::Channel<client::Msg>,
    command: &str,
) -> Result<(Vec<u8>, u32), SshError> {
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
    if exit_code != 0 {
        return Err(SshError::ExecFailed {
            command: command.to_string(),
            exit_code,
        });
    }
    Ok((stdout, exit_code))
}

/// 可在测试和集成测试中注入的 mock CommandRunner。
pub struct MockRunner {
    /// 预设响应列表：(命令前缀, exit_code, stdout)
    pub responses: std::sync::Mutex<Vec<(String, u32, Vec<u8>)>>,
    /// 记录 sftp_write 调用：(remote_path, bytes)
    pub sftp_writes: std::sync::Mutex<Vec<(String, Vec<u8>)>>,
    /// 记录所有 exec 调用的命令字符串
    pub exec_calls: std::sync::Mutex<Vec<String>>,
    /// 严格模式：未匹配命令返回 exit 127 Err（默认 false，未匹配返回 Ok）
    pub strict: bool,
}

impl MockRunner {
    pub fn new(responses: Vec<(String, u32, Vec<u8>)>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
            sftp_writes: std::sync::Mutex::new(Vec::new()),
            exec_calls: std::sync::Mutex::new(Vec::new()),
            strict: false,
        }
    }

    /// 创建严格模式 MockRunner（未匹配命令返回 exit 127 Err）。
    pub fn new_strict(responses: Vec<(String, u32, Vec<u8>)>) -> Self {
        Self {
            strict: true,
            ..Self::new(responses)
        }
    }

    /// 返回所有 exec 调用过的命令字符串副本。
    pub fn exec_log(&self) -> Vec<String> {
        self.exec_calls.lock().unwrap().clone()
    }

    /// 返回所有 sftp_write 调用的 (remote_path, bytes) 副本。
    pub fn sftp_log(&self) -> Vec<(String, Vec<u8>)> {
        self.sftp_writes.lock().unwrap().clone()
    }
}

#[async_trait]
impl CommandRunner for MockRunner {
    async fn exec(&self, command: &str) -> Result<(Vec<u8>, u32), SshError> {
        self.exec_calls.lock().unwrap().push(command.to_string());
        let mut responses = self.responses.lock().unwrap();
        if let Some(index) = responses
            .iter()
            .position(|(pattern, _, _)| command.starts_with(pattern.as_str()))
        {
            let (_, exit_code, stdout) = responses.remove(index);
            if exit_code != 0 {
                return Err(SshError::ExecFailed {
                    command: command.to_string(),
                    exit_code,
                });
            }
            Ok((stdout, exit_code))
        } else if self.strict {
            Err(SshError::ExecFailed {
                command: command.to_string(),
                exit_code: 127,
            })
        } else {
            // 非严格模式：未匹配命令返回 Ok([], 0)
            Ok((vec![], 0))
        }
    }

    async fn sftp_write(&self, remote_path: &str, bytes: &[u8]) -> Result<(), SshError> {
        self.sftp_writes
            .lock()
            .unwrap()
            .push((remote_path.to_string(), bytes.to_vec()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use russh::client::Handler;
    use super::*;

    #[test]
    fn test_expand_tilde_replaces_home() {
        // SAFETY: 单线程测试中修改环境变量，无并发风险
        unsafe { std::env::set_var("HOME", "/home/testuser") };
        let input = std::path::PathBuf::from("~/.ssh/id_rsa");
        let expanded = expand_tilde(&input);
        assert_eq!(
            expanded,
            std::path::PathBuf::from("/home/testuser/.ssh/id_rsa"),
            "~/前缀应被替换为 $HOME 路径"
        );
    }

    #[test]
    fn test_expand_tilde_no_tilde_unchanged() {
        let input = std::path::PathBuf::from("/absolute/path/key");
        let expanded = expand_tilde(&input);
        assert_eq!(expanded, input, "绝对路径应原样返回");
    }

    #[test]
    fn test_expand_tilde_missing_home_returns_input() {
        // SAFETY: 单线程测试中修改环境变量，无并发风险
        unsafe { std::env::remove_var("HOME") };
        let input = std::path::PathBuf::from("~/foo");
        let expanded = expand_tilde(&input);
        assert_eq!(expanded, input, "HOME 未设置时应原路径返回不 panic");
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_tofu_logs_fingerprint() {
        const TEST_PUBKEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti test@example";
        let public_key = russh::keys::PublicKey::from_openssh(TEST_PUBKEY)
            .expect("解析 OpenSSH 公钥");
        let mut handler = TofuHandler {
            accepted_keys: std::sync::Mutex::new(Vec::new()),
        };
        handler.check_server_key(&public_key).await.unwrap();
        assert!(logs_contain("[ssh][TOFU]"), "日志应含 [ssh][TOFU] 字串");
    }

    #[test]
    fn test_ssh_error_exec_failed_display() {
        let err = SshError::ExecFailed {
            command: "sudo -n true".to_string(),
            exit_code: 1,
        };
        let msg = err.to_string();
        assert!(msg.contains("sudo -n true"), "应含命令名: {msg}");
        assert!(msg.contains("exit 1"), "应含 exit 1: {msg}");
        assert!(msg.contains("SSH 命令执行失败"), "应含中文描述: {msg}");
    }

    #[test]
    fn test_ssh_error_variants_exist() {
        // 验证三个 variant 均可构造
        let _connect = SshError::Connect {
            host: "192.168.1.10".to_string(),
            source: russh::Error::NotAuthenticated,
        };
        let _exec = SshError::ExecFailed {
            command: "ls".to_string(),
            exit_code: 2,
        };
        let _sftp = SshError::SftpUpload {
            remote_path: "/opt/dm".to_string(),
            source: russh_sftp::client::error::Error::UnexpectedBehavior(
                "test".to_string(),
            ),
        };
    }

    #[tokio::test]
    async fn test_mock_runner_matching() {
        let runner = MockRunner::new(vec![("sudo -n true".to_string(), 0, vec![])]);
        let result = runner.exec("sudo -n true").await;
        assert!(result.is_ok(), "匹配命令应返回 Ok");
        let (stdout, exit_code) = result.unwrap();
        assert_eq!(stdout, vec![], "stdout 应为空");
        assert_eq!(exit_code, 0, "exit_code 应为 0");
    }

    #[tokio::test]
    async fn test_mock_runner_no_match_returns_127() {
        let runner = MockRunner::new_strict(vec![]);
        let result = runner.exec("some-command").await;
        assert!(result.is_err(), "严格模式未匹配命令应返回 Err");
        let err = result.unwrap_err();
        assert!(
            matches!(err, SshError::ExecFailed { exit_code: 127, .. }),
            "应返回 exit_code 127"
        );
    }
}
