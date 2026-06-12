use anyhow::Result;
use console::{style, Term};
use std::io::{BufRead, Write};

use crate::config::InstallConfig;

/// 状态消息级别，用于控制输出样式
pub enum StatusLevel {
    /// 成功状态
    Ok,
    /// 错误状态
    Error,
    /// 普通信息
    Info,
    /// 警告信息
    Warn,
}

/// 打印带样式的状态消息（[OK] / [ERROR] / [INFO] / [WARN]）。
///
/// 在非 TTY 环境（CI / 管道）下自动降级为纯文本输出。
pub fn print_status(level: StatusLevel, msg: &str) {
    let term = Term::stdout();
    let prefix = match level {
        StatusLevel::Ok => style("[OK]").green().bold(),
        StatusLevel::Error => style("[ERROR]").red().bold(),
        StatusLevel::Info => style("[INFO]").cyan().bold(),
        StatusLevel::Warn => style("[WARN]").yellow().bold(),
    };
    let _ = term.write_line(&format!("{} {}", prefix, msg));
}

/// 打印 dminit 不可修改参数并等待用户确认（INST-03）。
///
/// `skip=true` 时自动确认，不读取 stdin。
/// 关键约束（Pitfall 4）：curl | sh 管道场景 stdin 已被 curl 占用，
/// --defaults 模式下绝不调用 stdin().read_line()。
pub fn confirm_immutable_params(config: &InstallConfig, skip: bool) -> Result<()> {
    let term = Term::stdout();
    term.write_line(&format!(
        "{}",
        style("以下参数安装后不可修改：").yellow().bold()
    ))?;
    term.write_line(&format!("   PAGE_SIZE        : {}", config.page_size))?;
    let charset_name = match config.charset {
        0 => "GB18030",
        1 => "UTF-8",
        2 => "EUC-KR",
        _ => "UNKNOWN",
    };
    term.write_line(&format!("   CHARSET          : {}", charset_name))?;
    term.write_line(&format!(
        "   CASE_SENSITIVE   : {}",
        if config.case_sensitive { "Y" } else { "N" }
    ))?;
    term.write_line(&format!("   EXTENT_SIZE      : {}", config.extent_size))?;

    if skip {
        term.write_line("确认继续安装？[y/N] y (--defaults 自动确认)")?;
        return Ok(());
    }

    print!("确认继续安装？[y/N] ");
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input)?;
    if input.trim().to_lowercase() != "y" {
        anyhow::bail!("用户取消安装");
    }
    Ok(())
}
