use anyhow::Result;
use std::path::Path;

use crate::cli::ValidateArgs;
use crate::config::dw::DwClusterConfig;
use crate::config::ssh::SshTarget;
use crate::config::{ArchiveConfig, CommonConfig, InstallConfig, InstallerSource, LoadedSpecific};

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

fn print_dw_summary(path: &Path, common: &CommonConfig, cfg: &DwClusterConfig) {
    println!("配置文件: {} + dw.toml", path.display());
    println!("安装类型: 主备集群 (dw)");
    match &common.installer {
        InstallerSource::LocalFile(p) => println!("  安装包:     {}", p.display()),
        InstallerSource::Url(u) => println!("  安装包:     下载 {}", u),
        InstallerSource::Auto => println!("  安装包:     自动检测下载"),
    }
    println!("  oguid:      {}", cfg.oguid);
    println!("  节点数:     {}", cfg.nodes.len());
    for node in &cfg.nodes {
        println!(
            "\n  [{:?}] {} ({})",
            node.role, node.host, node.instance_name
        );
        println!("    安装路径:   {}", node.install_path);
        println!("    数据路径:   {}", node.data_path);
        println!(
            "    端口:       port={} mal_port={} dw_port={} inst_dw_port={}",
            node.port, node.mal_port, node.dw_port, node.inst_dw_port
        );
        println!(
            "    页大小:     {} KB / 字符集: {} / 簇大小: {}",
            node.page_size, node.charset, node.extent_size
        );
        println!("    SSH 用户:   {}", node.ssh.user);
    }
}

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

fn print_standalone_summary(path: &Path, common: &CommonConfig, cfg: &InstallConfig) {
    println!("配置文件: {} + standalone.toml", path.display());
    println!("安装类型: 单机 (standalone)");
    println!("\n[安装配置]");
    match &common.installer {
        InstallerSource::LocalFile(p) => println!("  安装包:     {}", p.display()),
        InstallerSource::Url(u) => println!("  安装包:     下载 {}", u),
        InstallerSource::Auto => println!("  安装包:     自动检测下载"),
    }
    println!("  安装路径:   {}", cfg.install_path);
    println!("  数据路径:   {}", cfg.data_path);
    println!("  实例名称:   {}", cfg.instance_name);
    println!("  端口:       {}", cfg.port);
    println!("  页大小:     {} KB", cfg.page_size);
    println!(
        "  字符集:     {} ({})",
        charset_name(cfg.charset),
        cfg.charset
    );
    println!("  区分大小写: {}", yn(cfg.case_sensitive));
    println!("  簇大小:     {}", cfg.extent_size);
    print_standalone_archive_section(cfg);
    if let Some(target) = &cfg.ssh_target {
        print_ssh_target_section(target);
    }
}

fn print_standalone_archive_section(cfg: &InstallConfig) {
    let default_path = format!("{}/arch（默认）", cfg.data_path);
    print_archive_section(&cfg.archive, &default_path);
}

fn print_ssh_target_section(target: &SshTarget) {
    println!("\n[SSH 远程目标]");
    println!("  主机: {}:{}", target.host, target.ssh_port);
    println!("  用户: {}", target.user);
    let auth = if target.password.is_some() {
        "密码（已配置）"
    } else {
        "密码（安装时将提示输入）"
    };
    println!("  认证: {}", auth);
}

fn print_archive_section(arch: &ArchiveConfig, default_path: &str) {
    println!("\n[归档配置]");
    match &arch.arch_path {
        Some(p) => println!("  归档目录:   {}", p),
        None => println!("  归档目录:   {}", default_path),
    }
    println!("  文件大小:   {} MB", arch.file_size);
    match arch.space_limit {
        Some(0) => println!("  空间上限:   无限制"),
        Some(limit) => println!("  空间上限:   {} MB", limit),
        None => println!("  空间上限:   自动（磁盘总容量的 20%）"),
    }
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
