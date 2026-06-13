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
