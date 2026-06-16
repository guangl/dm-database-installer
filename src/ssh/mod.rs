mod error;
mod local;
#[cfg(test)]
mod mock;
mod runner;
mod session;

pub use error::SshError;
pub use local::LocalRunner;
#[cfg(test)]
pub use mock::MockRunner;
pub use runner::CommandRunner;
pub use session::SshSession;

/// 对 shell 参数进行单引号转义，防止命令注入。
pub(crate) fn shell_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "'\\''"))
}
