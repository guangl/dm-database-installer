use anyhow::Result;
use async_trait::async_trait;

use super::error::SshError;
use super::runner::CommandRunner;

/// 可在测试中注入的 mock CommandRunner。
pub struct MockRunner {
    /// 预设响应列表：(命令前缀, exit_code, stdout)，匹配后移除。
    pub responses: std::sync::Mutex<Vec<(String, u32, Vec<u8>)>>,
    /// 记录 sftp_write 调用：(remote_path, bytes)
    pub sftp_writes: std::sync::Mutex<Vec<(String, Vec<u8>)>>,
    /// 记录所有 exec 调用的命令字符串
    pub exec_calls: std::sync::Mutex<Vec<String>>,
    /// 严格模式：未匹配命令返回 exit 127 Err（默认 false）
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

    pub fn new_strict(responses: Vec<(String, u32, Vec<u8>)>) -> Self {
        Self { strict: true, ..Self::new(responses) }
    }

    pub fn exec_log(&self) -> Vec<String> {
        self.exec_calls.lock().unwrap().clone()
    }

    pub fn sftp_log(&self) -> Vec<(String, Vec<u8>)> {
        self.sftp_writes.lock().unwrap().clone()
    }
}

#[async_trait]
impl CommandRunner for MockRunner {
    async fn exec(&self, command: &str) -> Result<(Vec<u8>, u32), SshError> {
        self.exec_calls.lock().unwrap().push(command.to_string());
        let mut responses = self.responses.lock().unwrap();
        if let Some(idx) = responses
            .iter()
            .position(|(pattern, _, _)| command.starts_with(pattern.as_str()))
        {
            let (_, exit_code, stdout) = responses.remove(idx);
            if exit_code != 0 {
                let output = String::from_utf8_lossy(&stdout).trim().to_string();
                return Err(SshError::ExecFailed { command: command.to_string(), exit_code, output });
            }
            Ok((stdout, exit_code))
        } else if self.strict {
            Err(SshError::ExecFailed { command: command.to_string(), exit_code: 127, output: String::new() })
        } else {
            Ok((vec![], 0))
        }
    }

    async fn sftp_write(&self, remote_path: &str, bytes: &[u8]) -> Result<(), SshError> {
        self.sftp_writes.lock().unwrap().push((remote_path.to_string(), bytes.to_vec()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_error_exec_failed_display() {
        let err = SshError::ExecFailed { command: "sudo -n true".to_string(), exit_code: 1, output: "Permission denied".to_string() };
        let msg = err.to_string();
        assert!(msg.contains("sudo -n true"));
        assert!(msg.contains("exit 1"));
        assert!(msg.contains("SSH 命令执行失败"));
        assert!(msg.contains("Permission denied"));
    }

    #[test]
    fn test_ssh_error_variants_exist() {
        let _c = SshError::Connect {
            host: "192.168.1.10".to_string(),
            source: russh::Error::NotAuthenticated,
        };
        let _e = SshError::ExecFailed { command: "ls".to_string(), exit_code: 2, output: String::new() };
        let _s = SshError::SftpUpload {
            remote_path: "/opt/dm".to_string(),
            source: russh_sftp::client::error::Error::UnexpectedBehavior("test".to_string()),
        };
    }

    #[tokio::test]
    async fn test_mock_runner_matching() {
        let runner = MockRunner::new(vec![("sudo -n true".to_string(), 0, vec![])]);
        let (stdout, exit_code) = runner.exec("sudo -n true").await.unwrap();
        assert_eq!(stdout, Vec::<u8>::new());
        assert_eq!(exit_code, 0);
    }

    #[tokio::test]
    async fn test_mock_runner_no_match_returns_127() {
        let runner = MockRunner::new_strict(vec![]);
        let err = runner.exec("some-command").await.unwrap_err();
        assert!(matches!(err, SshError::ExecFailed { exit_code: 127, .. }));
    }
}
