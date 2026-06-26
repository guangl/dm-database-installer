use serde::Deserialize;
use std::path::PathBuf;

/// SSH 认证凭据（密钥或密码，至少一种）。
#[derive(Debug, Deserialize, Clone)]
pub struct SshCredentials {
    pub user: String,
    pub identity_file: Option<PathBuf>,
    #[serde(skip_serializing, default)]
    pub password: Option<String>,
}

/// 单机 SSH 远程安装目标（standalone.toml 可选 [ssh_target] 块）。
/// password 为 None 时运行时提示输入。
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct SshTarget {
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    pub user: String,
    pub password: Option<String>,
    /// 连接失败时的最大重试次数，默认 3
    #[serde(default = "default_ssh_max_retries")]
    pub max_retries: u32,
    /// 每次重试前的等待秒数，默认 5
    #[serde(default = "default_ssh_retry_interval_secs")]
    pub retry_interval_secs: u64,
}

fn default_ssh_port() -> u16 {
    22
}
fn default_ssh_max_retries() -> u32 {
    3
}
fn default_ssh_retry_interval_secs() -> u64 {
    5
}

/// 校验集群节点的 SSH 凭据：必须提供 identity_file 或 password 之一。
/// dw/dpc 两种集群配置的节点校验共用此规则。
pub(crate) fn validate_node_ssh_credentials(host: &str, ssh: &SshCredentials) -> anyhow::Result<()> {
    if ssh.identity_file.is_none() && ssh.password.is_none() {
        anyhow::bail!(
            "配置验证失败: 节点 {} 的 ssh 配置必须提供 identity_file 或 password 之一",
            host
        );
    }
    Ok(())
}
