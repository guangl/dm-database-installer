pub mod archive;
pub mod backup;
pub mod env_setup;
pub mod init;
pub mod package;
pub mod param_tune;
pub mod preflight;
pub mod service;
pub mod silent_install;
pub mod sql_log;

use crate::ssh::shell_quote;

/// 构造以 dmdba 身份运行 disql 脚本的 shell 命令。
///
/// disql 官方文档（disql-script.html）未提供 shell `<` 重定向方式执行脚本；
/// 官方支持的是 START / `` ` `` / `@` / `@@` 几种脚本命令，其中反引号前缀
/// （`` `脚本路径 ``，紧跟路径无空格）可直接作为 disql 命令行参数传入。
pub(crate) fn disql_script_cmd(disql_bin: &str, conn: &str, script_path: &str) -> String {
    let script_arg = format!("`{script_path}");
    let inner = format!(
        "{} {} {}",
        shell_quote(disql_bin),
        shell_quote(conn),
        shell_quote(&script_arg),
    );
    format!("su - dmdba -c {}", shell_quote(&inner))
}
