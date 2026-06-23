use anyhow::Result;
use std::path::Path;

use crate::cli::ValidateArgs;
use crate::config::dw::{DwClusterConfig, DwNode};
use crate::config::ssh::SshTarget;
use crate::config::{ArchiveConfig, BackupConfig, CommonConfig, InstallConfig, InstallerSource, LoadedSpecific};

pub async fn run(args: &ValidateArgs) -> Result<()> {
    let config_path = resolve_common_config_path(args.config.as_deref());
    let loaded = crate::config::load_config_from(&config_path)?;

    match &loaded.specific {
        LoadedSpecific::Standalone(cfg) => {
            print_standalone_summary(&config_path, &loaded.common, cfg)
        }
        LoadedSpecific::Dw(cluster) => print_dw_summary(&config_path, &loaded.common, cluster),
    }
    println!("\n✓ 配置解析成功");
    Ok(())
}

// ── 辅助输出 ────────────────────────────────────────────────────────

const W: usize = 10; // key 列宽

fn section(title: &str) {
    println!("\n{title}");
    println!("{}", "─".repeat(title.chars().count().max(36)));
}

fn kv(key: &str, val: &str) {
    println!("  {key:<W$}  {val}");
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
    println!("主备集群  {} + dw.toml", path.display());

    section("集群配置");
    kv("安装包", &installer_line(&common.installer));
    kv("OGUID", &cfg.oguid.to_string());
    kv("切换模式", &format!(
        "{}（{}）",
        cfg.dw_mode.as_str(),
        if cfg.dw_mode == crate::config::dw::DwMode::Auto {
            "故障自动切换"
        } else {
            "人工介入切换"
        }
    ));
    kv("监视器", &format!(
        "{}（MON_DW_CONFIRM={}）",
        if cfg.mon_confirm { "确认监视，参与仲裁" } else { "通知监视，不参与仲裁" },
        cfg.mon_confirm as u8,
    ));

    for (i, node) in cfg.nodes.iter().enumerate() {
        print_dw_node(i + 1, cfg.nodes.len(), node);
    }

    if let Some(primary) = cfg.nodes.iter().find(|n| n.role == crate::config::dw::NodeRole::Primary) {
        if let Some(b) = &primary.backup {
            print_backup_section(&format!("备份作业  {}（{}）", primary.instance_name, primary.host), b);
        }
    }
}

fn print_dw_node(idx: usize, total: usize, node: &DwNode) {
    let role = format!("{:?}", node.role).to_uppercase();
    section(&format!("节点 [{idx}/{total}]  {role}  {}  {}", node.host, node.instance_name));
    kv("安装路径", &node.install_path);
    kv("数据路径", &node.data_path);
    kv("归档目录", &node.resolve_arch_path());
    kv("端口", &format!(
        "DB={}  MAL={}  DW={}  INST_DW={}",
        node.port, node.mal_port, node.dw_port, node.inst_dw_port
    ));
    kv("数据库", &format!(
        "页大小={}KB  字符集={}  簇大小={}  大小写={}",
        node.page_size,
        charset_name(node.charset),
        node.extent_size,
        yn(node.case_sensitive),
    ));
    kv("SSH", &format!("{}@{}", node.ssh.user, node.host));
}

// ── 单机 ────────────────────────────────────────────────────────────

fn print_standalone_summary(path: &Path, common: &CommonConfig, cfg: &InstallConfig) {
    println!("单机安装  {} + standalone.toml", path.display());

    section("安装配置");
    kv("安装包", &installer_line(&common.installer));
    kv("安装路径", &cfg.install_path);
    kv("数据路径", &cfg.data_path);

    section(&format!("数据库实例  {}", cfg.instance_name));
    kv("端口", &cfg.port.to_string());
    kv("页大小", &format!("{} KB", cfg.page_size));
    kv("字符集", &format!("{} ({})", charset_name(cfg.charset), cfg.charset));
    kv("大小写", yn(cfg.case_sensitive));
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
