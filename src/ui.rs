use console::style;

use crate::config::{InstallConfig, resolve_arch_path};

/// 供需要手动拼接 ANSI 转义码的场景使用（如 `validate.rs` 里要做显示宽度对齐计算）。
/// 是否启用颜色由 `console` crate 统一判断（TTY 检测 + `NO_COLOR`/`CLICOLOR_FORCE` 环境变量）。
pub struct Colors {
    pub green: &'static str,
    pub yellow: &'static str,
    pub cyan: &'static str,
    pub dim: &'static str,
    pub bold: &'static str,
    pub reset: &'static str,
}

pub fn colors() -> Colors {
    if console::colors_enabled() {
        Colors {
            green: "\x1b[32m",
            yellow: "\x1b[33m",
            cyan: "\x1b[36m",
            dim: "\x1b[2m",
            bold: "\x1b[1m",
            reset: "\x1b[0m",
        }
    } else {
        Colors {
            green: "",
            yellow: "",
            cyan: "",
            dim: "",
            bold: "",
            reset: "",
        }
    }
}

pub fn log_ok(msg: &str) {
    println!("{}   {}", style("[OK]").green(), msg);
}

pub fn log_warn(msg: &str) {
    println!("{} {}", style("[WARN]").yellow(), msg);
}

pub fn log_info(msg: &str) {
    println!("  ·  {}", msg);
}

/// 本机当前日期，格式 YYYYMMDD（用于报告文件名）。
pub fn today_yyyymmdd() -> String {
    std::process::Command::new("date")
        .arg("+%Y%m%d")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn step_header(title: &str) {
    println!(
        "\n{}",
        style(format!(
            "── {} ──────────────────────────────────────────────",
            title
        ))
        .yellow()
    );
}

pub fn step_footer() {
    println!(
        "{}",
        style("──────────────────────────────────────────────────────────────").yellow()
    );
}

pub fn check_ok(label: &str, detail: &str) {
    let mark = style("✓").green();
    if detail.is_empty() {
        println!("  {}  {}", mark, label);
    } else {
        println!("  {}  {}: {}", mark, label, detail);
    }
}

pub fn check_warn(label: &str, detail: &str) {
    let mark = style("⚠").yellow();
    if detail.is_empty() {
        println!("  {}  {}", mark, label);
    } else {
        println!("  {}  {}: {}", mark, label, detail);
    }
}

pub fn print_banner() {
    println!(
        "{}",
        style("╔══════════════════════════════════════════════════════════════╗").yellow()
    );
    println!(
        "{}",
        style("║  ⚠  此工具会修改内核参数、关闭 SELinux 和防火墙。            ║").yellow()
    );
    println!(
        "{}",
        style("║  ⚠  安装完成后会自动开启本地归档（ARCHIVELOG）。            ║").yellow()
    );
    println!(
        "{}",
        style("╚══════════════════════════════════════════════════════════════╝").yellow()
    );
    println!();
}

pub fn print_success(
    config: &InstallConfig,
    sysdba_pwd: &str,
    sysauditor_pwd: &str,
    dm_version: Option<&str>,
) {
    let arch_path = resolve_arch_path(&config.archive, &config.data_path);
    let charset_name = match config.charset {
        0 => "GB18030",
        1 => "UTF-8",
        2 => "EUC-KR",
        _ => "未知",
    };
    let arch_space = match config.archive.space_limit {
        Some(0) => "不限".to_string(),
        Some(limit) => format!("{} MB", limit),
        None => "自动（磁盘总容量的 20%）".to_string(),
    };

    println!();
    println!("{}", style("✓ 达梦数据库安装完成").green());
    println!();
    println!("  安装路径    : {}", config.install_path);
    println!("  数据路径    : {}/DAMENG", config.data_path);
    println!("  监听端口    : {}", config.port);
    println!();
    println!("╔═══════════════════════════════════════════════════╗");
    println!("║            达梦数据库初始化参数                   ║");
    println!("╠═══════════════════════════════════════════════════╣");
    println!("║  数据库版本: {:<37}║", dm_version.unwrap_or("未知"));
    println!("║  数据库名  : {:<37}║", "DAMENG");
    println!("║  实例名    : {:<37}║", config.instance_name);
    println!("║  页大小    : {:<37}║", format!("{} KB", config.page_size));
    println!("║  簇大小    : {:<37}║", config.extent_size);
    println!("║  字符集    : {:<37}║", charset_name);
    println!(
        "║  大小写敏感: {:<37}║",
        if config.case_sensitive { "Y" } else { "N" }
    );
    println!("╠═══════════════════════════════════════════════════╣");
    println!("║  SYSDBA     密码: {:<32}║", sysdba_pwd);
    println!("║  SYSAUDITOR 密码: {:<32}║", sysauditor_pwd);
    println!("╠═══════════════════════════════════════════════════╣");
    println!("║  首次登录后请立即修改密码                         ║");
    println!("╚═══════════════════════════════════════════════════╝");
    println!();
    println!("  归档路径    : {}", arch_path);
    println!("  归档文件大小: {} MB", config.archive.file_size);
    println!("  归档空间上限: {}", arch_space);
    println!();
    println!(
        "  连接测试  : {}/bin/disql SYSDBA/'{}'@localhost:{}",
        config.install_path, sysdba_pwd, config.port
    );
    println!(
        "  查看状态  : systemctl status DmService{}.service",
        config.instance_name
    );
    println!();
}

/// 打印一组配置建议（与安装模式无关的通用渲染）。各安装模式自行收集建议内容，
/// 例如单机模式见 `install::advisory::standalone_advisories`。
pub fn print_advisories(advisories: &[String]) {
    if advisories.is_empty() {
        return;
    }
    println!(
        "{}",
        style("╔══════════════════════════════════════════════════════════════╗").yellow()
    );
    println!("{}", style("║  ⚠  配置建议").yellow());
    for a in advisories {
        println!("{}    - {}", style("║").yellow(), a);
    }
    println!(
        "{}",
        style("╚══════════════════════════════════════════════════════════════╝").yellow()
    );
    println!();
}
