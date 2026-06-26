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

use crate::config::InstallConfig;
use crate::ssh::{CommandRunner, shell_quote};

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

/// 将 SQL 脚本写入远端临时文件，以 dmdba 身份通过 disql 执行，完成后清理脚本文件。
///
/// `write_err_ctx`/`exec_err_ctx` 是写入/执行失败时附加的错误提示前缀，
/// 各调用方据此区分是写脚本失败还是执行失败。
pub(crate) async fn execute_sql_script(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
    sql_path: &str,
    sql: &str,
    write_err_ctx: &str,
    exec_err_ctx: &str,
) -> anyhow::Result<()> {
    runner
        .sftp_write(sql_path, sql.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("{write_err_ctx}: {e}"))?;

    let disql = format!("{}/bin/disql", config.install_path);
    let conn = format!("SYSDBA/{}@localhost:{}", sysdba_pwd, config.port);
    let cmd = disql_script_cmd(&disql, &conn, sql_path);
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("{exec_err_ctx}: {e}"))?;

    let _ = runner.exec(&format!("rm -f {}", shell_quote(sql_path))).await;
    Ok(())
}
