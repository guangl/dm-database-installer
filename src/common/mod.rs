pub mod download;
pub mod ssh;
pub mod sysinfo;

/// 对 shell 参数进行单引号转义，防止命令注入。
/// 所有用户可控路径和实例名在拼入 shell 命令前必须经过此函数。
pub(crate) fn shell_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "'\\''"))
}
