use anyhow::Result;
use std::path::Path;
use unicode_width::UnicodeWidthStr;

use crate::cli::ValidateArgs;
use crate::config::dw::{DwClusterConfig, DwNode, NodeRole, StandbyMode};
use crate::config::ssh::SshTarget;
use crate::config::{ArchiveConfig, BackupConfig, CommonConfig, InstallConfig, InstallerSource, LoadedSpecific};
use crate::ui::colors;

pub async fn run(args: &ValidateArgs) -> Result<()> {
    let config_path = resolve_common_config_path(args.config.as_deref());
    let loaded = crate::config::load_config_from(&config_path)?;

    match &loaded.specific {
        LoadedSpecific::Standalone(cfg) => {
            print_standalone_summary(&config_path, &loaded.common, cfg)
        }
        LoadedSpecific::Dw(cluster) => print_dw_summary(&config_path, &loaded.common, cluster),
    }
    let c = colors();
    println!("\n{}✓ 配置解析成功{}", c.green, c.reset);
    Ok(())
}

// ── 辅助输出 ────────────────────────────────────────────────────────

/// "key：" 列对齐宽度（按显示宽度计算，CJK=2 列）。覆盖本文件最长的
/// "key：" 组合（如"发送延迟阈值："显示宽度 14）并留 2 列间隔。
const ALIGN_WIDTH: usize = 16;

/// 三列模式（`kv3`）中"说明"列的对齐宽度，覆盖最长说明文本
/// （如"同步备库需先应用日志再回应"显示宽度 26）并留 2 列间隔。
const DESC_WIDTH: usize = 28;

/// 去除 ANSI 颜色转义序列，仅用于计算显示宽度（`section` 标题可能内嵌颜色码，
/// 直接对带转义码的字符串调用 `.width()` 会把转义字符也计入宽度，导致下划线长度算错）。
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn section(title: &str) {
    let c = colors();
    println!("\n{}{}{}{}", c.bold, c.cyan, title, c.reset);
    println!("{}{}{}", c.dim, "─".repeat(strip_ansi(title).width().max(36)), c.reset);
}

fn kv(key: &str, val: &str) {
    let c = colors();
    let prefix = format!("{key}：");
    let pad = " ".repeat(ALIGN_WIDTH.saturating_sub(prefix.width()));
    println!("  {}{prefix}{}{pad}{val}", c.cyan, c.reset);
}

/// ini 配置项名对齐宽度（全是 ASCII，按字符数即显示宽度）。覆盖本文件最长的参数名
/// （如 `MAL_CONN_FAIL_INTERVAL`/`INST_SERVICE_IP_CHECK`，22 个字符）。
const INI_PARAM_WIDTH: usize = 22;

/// 格式化一条 `PARAM = value`，PARAM 固定宽度左对齐，使同一份输出里所有"="对齐。
fn ini_kv(param: &str, value: &str) -> String {
    format!("{param:<INI_PARAM_WIDTH$} = {value}")
}

/// 三列输出：key  说明文本  对应 ini 配置项（用 `ini_kv` 生成，"=" 对齐），三列各自独立对齐，
/// 用于 dmwatcher.ini/dmmal.ini/dmarch.ini/dmmonitor.ini 等"人类说明 + 原始配置项"并列的场景。
fn kv3(key: &str, desc: &str, ini_line: &str) {
    let c = colors();
    let prefix = format!("{key}：");
    let pad1 = " ".repeat(ALIGN_WIDTH.saturating_sub(prefix.width()));
    let pad2 = " ".repeat(DESC_WIDTH.saturating_sub(desc.width()));
    println!(
        "  {}{prefix}{}{pad1}{desc}{pad2}{}{ini_line}{}",
        c.cyan, c.reset, c.dim, c.reset,
    );
}

fn installer_line(src: &InstallerSource) -> String {
    match src {
        InstallerSource::LocalFile(p) => format!("本地文件  {}", p.display()),
        InstallerSource::Url(u) => format!("下载  {}", u),
        InstallerSource::Auto => "自动检测下载".to_string(),
    }
}

// ── 主备集群 ────────────────────────────────────────────────────────

fn print_dw_summary(path: &Path, common: &CommonConfig, cfg: &DwClusterConfig) {
    let c = colors();
    println!("{}{}主备集群{}  {} + dw.toml", c.bold, c.yellow, c.reset, path.display());

    section("集群配置");
    kv("安装包", &installer_line(&common.installer));
    kv("OGUID", &cfg.oguid.to_string());

    section("守护配置（dmwatcher.ini）");
    let w = &cfg.watcher;
    kv("切换模式", &format!(
        "{}（{}）",
        w.dw_mode.as_str(),
        if w.dw_mode == crate::config::dw::DwMode::Auto { "故障自动切换" } else { "人工介入切换" }
    ));
    kv3("故障确认", &format!("{} 秒", w.dw_error_time), &ini_kv("DW_ERROR_TIME", &w.dw_error_time.to_string()));
    kv3("实例超时", &format!("{} 秒", w.inst_error_time), &ini_kv("INST_ERROR_TIME", &w.inst_error_time.to_string()));
    kv3("恢复等待", &format!("{} 秒", w.inst_recover_time), &ini_kv("INST_RECOVER_TIME", &w.inst_recover_time.to_string()));
    kv3(
        "强制超时",
        &if w.dw_open_force_timeout == 0 { "永久等待".to_string() } else { format!("{} 秒", w.dw_open_force_timeout) },
        &ini_kv("DW_OPEN_FORCE_TIMEOUT", &w.dw_open_force_timeout.to_string()),
    );
    kv3(
        "无监视器",
        &format!("{}强制 Open", if w.dw_failover_force != 0 { "允许" } else { "禁止" }),
        &ini_kv("DW_FAILOVER_FORCE", &w.dw_failover_force.to_string()),
    );
    kv3(
        "断链重连",
        match w.dw_reconnect {
            0 => "不重连",
            1 => "重连后继续守护",
            2 => "重连后降为 OPEN",
            _ => "未知",
        },
        &ini_kv("DW_RECONNECT", &w.dw_reconnect.to_string()),
    );
    kv3(
        "自动重启",
        &format!(
            "{}，最多 {} 次",
            if w.inst_auto_restart != 0 { "是" } else { "否" },
            if w.inst_restart_cnt == 0 { "不限".to_string() } else { w.inst_restart_cnt.to_string() },
        ),
        &format!(
            "{}  {}",
            ini_kv("INST_AUTO_RESTART", &w.inst_auto_restart.to_string()),
            ini_kv("INST_RESTART_CNT", &w.inst_restart_cnt.to_string()),
        ),
    );
    kv3(
        "IP 可达检测",
        if w.inst_service_ip_check != 0 { "开启" } else { "关闭" },
        &ini_kv("INST_SERVICE_IP_CHECK", &w.inst_service_ip_check.to_string()),
    );
    kv3(
        "发送延迟阈值",
        &if w.rlog_send_threshold == 0 { "不告警".to_string() } else { format!("{} 秒", w.rlog_send_threshold) },
        &ini_kv("RLOG_SEND_THRESHOLD", &w.rlog_send_threshold.to_string()),
    );
    kv3(
        "应用延迟阈值",
        &if w.rlog_apply_threshold == 0 { "不告警".to_string() } else { format!("{} 秒", w.rlog_apply_threshold) },
        &ini_kv("RLOG_APPLY_THRESHOLD", &w.rlog_apply_threshold.to_string()),
    );

    section("MAL 通信配置（dmmal.ini）");
    let m = &cfg.mal;
    kv3(
        "链路检测",
        &if m.mal_check_interval == 0 { "禁用".to_string() } else { format!("{} 秒", m.mal_check_interval) },
        &ini_kv("MAL_CHECK_INTERVAL", &m.mal_check_interval.to_string()),
    );
    kv3("失败阈值", &format!("{} 秒", m.mal_conn_fail_interval), &ini_kv("MAL_CONN_FAIL_INTERVAL", &m.mal_conn_fail_interval.to_string()));
    kv3("登录超时", &format!("{} 秒", m.mal_login_timeout), &ini_kv("MAL_LOGIN_TIMEOUT", &m.mal_login_timeout.to_string()));
    kv3("缓冲区", &format!("{} MB", m.mal_buf_size), &ini_kv("MAL_BUF_SIZE", &m.mal_buf_size.to_string()));
    kv3(
        "系统上限",
        &if m.mal_sys_buf_size == 0 { "无限制".to_string() } else { format!("{} MB", m.mal_sys_buf_size) },
        &ini_kv("MAL_SYS_BUF_SIZE", &m.mal_sys_buf_size.to_string()),
    );
    kv3(
        "压缩级别",
        &if m.mal_compress_level == 0 {
            "不压缩".to_string()
        } else if m.mal_compress_level <= 9 {
            format!("lz 压缩 level={}", m.mal_compress_level)
        } else {
            "snappy".to_string()
        },
        &ini_kv("MAL_COMPRESS_LEVEL", &m.mal_compress_level.to_string()),
    );

    section("归档配置（dmarch.ini）");
    let a = &cfg.arch;
    kv3(
        "等待应用",
        if a.arch_wait_apply != 0 { "同步备库需先应用日志再回应" } else { "不等待应用直接回应" },
        &ini_kv("ARCH_WAIT_APPLY", &a.arch_wait_apply.to_string()),
    );
    kv3(
        "保留时长",
        &if a.arch_reserve_time == 0 { "不自动清理".to_string() } else { format!("{} 分钟", a.arch_reserve_time) },
        &ini_kv("ARCH_RESERVE_TIME", &a.arch_reserve_time.to_string()),
    );
    kv3(
        "发送策略",
        if a.arch_send_policy == 0 { "立即等待备库响应" } else { "先写本地再发送" },
        &ini_kv("ARCH_SEND_POLICY", &a.arch_send_policy.to_string()),
    );
    kv3("恢复检测", &format!("{} 秒", a.arch_recover_time), &ini_kv("ARCH_RECOVER_TIME", &a.arch_recover_time.to_string()));
    kv3("文件大小", &format!("{} MB", a.arch_file_size), &ini_kv("ARCH_FILE_SIZE", &a.arch_file_size.to_string()));
    kv3(
        "空间上限",
        &match a.arch_space_limit {
            Some(0) => "无限制".to_string(),
            Some(limit) => format!("{} MB", limit),
            None => "自动（磁盘总容量的 20%，探测失败时默认 20480 MB）".to_string(),
        },
        &ini_kv("ARCH_SPACE_LIMIT", &match a.arch_space_limit {
            Some(limit) => limit.to_string(),
            None => "自动".to_string(),
        }),
    );

    print_dw_monitor_section(cfg);

    for (i, node) in cfg.nodes.iter().enumerate() {
        print_dw_node(i + 1, cfg.nodes.len(), node);
    }

    if let Some(primary) = cfg.nodes.iter().find(|n| n.role == NodeRole::Primary)
        && let Some(b) = &primary.backup
    {
        print_backup_section(&format!("备份作业  {}（{}）", primary.instance_name, primary.host), b);
    }
}

fn print_dw_monitor_section(cfg: &DwClusterConfig) {
    section("监视器配置（dmmonitor.ini）");
    let monitor = cfg.monitor_node();
    kv("运行节点", &format!("{}（{}）", monitor.instance_name, monitor.host));
    kv3(
        "仲裁模式",
        if cfg.mon_confirm { "确认监视，参与仲裁" } else { "通知监视，不参与仲裁" },
        &ini_kv("MON_DW_CONFIRM", &(cfg.mon_confirm as u8).to_string()),
    );
    kv3("OGUID", &cfg.oguid.to_string(), &ini_kv("MON_INST_OGUID", &cfg.oguid.to_string()));
    let mon = &cfg.monitor;
    kv3("日志路径", &mon.mon_log_path, &ini_kv("MON_LOG_PATH", &mon.mon_log_path));
    kv3("日志间隔", &format!("{} 秒", mon.mon_log_interval), &ini_kv("MON_LOG_INTERVAL", &mon.mon_log_interval.to_string()));
    kv3("单文件大小", &format!("{} MB", mon.mon_log_file_size), &ini_kv("MON_LOG_FILE_SIZE", &mon.mon_log_file_size.to_string()));
    kv3(
        "日志空间上限",
        &if mon.mon_log_space_limit == 0 { "无限制".to_string() } else { format!("{} MB", mon.mon_log_space_limit) },
        &ini_kv("MON_LOG_SPACE_LIMIT", &mon.mon_log_space_limit.to_string()),
    );
    for node in cfg.nodes.iter().filter(|n| n.role == NodeRole::Primary || n.sync_mode == StandbyMode::Realtime) {
        kv("MON_DW_IP", &format!(
            "{}:{}  {}（{:?}）",
            node.host, node.dw_port, node.instance_name, node.role,
        ));
    }
    let excluded: Vec<&DwNode> = cfg
        .nodes
        .iter()
        .filter(|n| n.role == NodeRole::Standby && n.sync_mode != StandbyMode::Realtime)
        .collect();
    if !excluded.is_empty() {
        let names: Vec<String> = excluded
            .iter()
            .map(|n| format!("{}（{:?}）", n.instance_name, n.sync_mode))
            .collect();
        kv("不参与仲裁", &names.join("、"));
    }
}

fn print_dw_node(idx: usize, total: usize, node: &DwNode) {
    let c = colors();
    let role_color = if node.role == NodeRole::Primary { c.green } else { c.yellow };
    let role = format!("{:?}", node.role).to_uppercase();
    section(&format!(
        "节点 [{idx}/{total}]  {role_color}{role}{reset}  {}  {}",
        node.host, node.instance_name, reset = c.reset,
    ));
    kv("安装路径", &node.install_path);
    kv("数据路径", &node.data_path);
    kv("归档目录", &node.resolve_arch_path());
    if node.role == NodeRole::Standby {
        kv("备库类型", node.sync_mode.arch_type());
        if node.sync_mode == StandbyMode::Async {
            kv("归档定时器", &node.arch_timer_name);
        }
    }
    kv("端口", &format!(
        "DB={}  MAL={}  DW={}  INST_DW={}",
        node.port, node.mal_port, node.dw_port, node.inst_dw_port
    ));
    kv("数据库", &format!(
        "页大小={}KB  字符集={}  簇大小={}  大小写敏感={}",
        node.page_size,
        charset_name(node.charset),
        node.extent_size,
        yn(node.case_sensitive),
    ));
    kv("SSH", &format!("{}@{}", node.ssh.user, node.host));
}

// ── 单机 ────────────────────────────────────────────────────────────

fn print_standalone_summary(path: &Path, common: &CommonConfig, cfg: &InstallConfig) {
    let c = colors();
    println!("{}{}单机安装{}  {} + standalone.toml", c.bold, c.yellow, c.reset, path.display());

    section("安装配置");
    kv("安装包", &installer_line(&common.installer));
    kv("安装路径", &cfg.install_path);
    kv("数据路径", &cfg.data_path);

    section(&format!("数据库实例  {}", cfg.instance_name));
    kv("端口", &cfg.port.to_string());
    kv("页大小", &format!("{} KB", cfg.page_size));
    kv("字符集", &format!("{} ({})", charset_name(cfg.charset), cfg.charset));
    kv("大小写敏感", yn(cfg.case_sensitive));
    kv("簇大小", &cfg.extent_size.to_string());

    print_arch_section(&cfg.archive, &format!("{}/arch", cfg.data_path));
    print_backup_section("备份作业", &cfg.backup);

    if let Some(target) = &cfg.ssh_target {
        print_ssh_section(target);
    }
}

fn print_arch_section(arch: &ArchiveConfig, default_path: &str) {
    section("归档配置");
    kv("归档目录", arch.arch_path.as_deref().unwrap_or(default_path));
    kv("文件大小", &format!("{} MB", arch.file_size));
    kv("空间上限", &match arch.space_limit {
        Some(0) => "无限制".to_string(),
        Some(limit) => format!("{} MB", limit),
        None => "自动（磁盘总容量的 20%）".to_string(),
    });
}

fn print_backup_section(title: &str, b: &BackupConfig) {
    section(title);
    kv("备份目录", b.backup_path.as_deref().unwrap_or("（未配置）"));
    kv("保留天数", &format!("{} 天", b.retain_days));
    kv("全量备份", &format!("每 {} 天  {}", b.full_backup_interval_days, b.full_backup_time));
    kv("增量备份", &format!("每天  {}", b.incr_backup_time));
    kv("清理时间", &b.clean_time);
}

fn print_ssh_section(target: &SshTarget) {
    section("SSH 远程目标");
    kv("主机", &format!("{}:{}", target.host, target.ssh_port));
    kv("用户", &target.user);
    kv("认证", if target.password.is_some() {
        "密码（已配置）"
    } else {
        "密码（安装时将提示输入）"
    });
}

// ── 公共函数 ────────────────────────────────────────────────────────

fn resolve_common_config_path(input: Option<&Path>) -> std::path::PathBuf {
    let path = match input {
        None => return std::path::PathBuf::from(crate::config::CONFIG_FILE),
        Some(p) => p,
    };
    if let Some(name) = path.file_name().and_then(|f| f.to_str())
        && name == "standalone.toml"
    {
        let dir = path.parent().unwrap_or(Path::new("."));
        let common = dir.join(crate::config::CONFIG_FILE);
        println!(
            "提示: standalone.toml 是特有配置文件，自动切换到 {} 进行验证",
            common.display()
        );
        return common;
    }
    path.to_path_buf()
}

fn charset_name(charset: u8) -> &'static str {
    match charset {
        0 => "GB18030",
        1 => "UTF-8",
        2 => "EUC-KR",
        _ => "未知",
    }
}

fn yn(b: bool) -> &'static str {
    if b { "是" } else { "否" }
}
