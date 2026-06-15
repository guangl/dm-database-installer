use anyhow::Result;
use std::path::Path;

use crate::cli::ValidateArgs;
use crate::common::ssh::{CommandRunner, SshSession};
use crate::config::cluster::{ClusterSpecificConfig, NodeConfig, NodeRole};
use crate::config::ssh::{SshCredentials, SshTarget};
use crate::config::{ArchiveConfig, CommonConfig, InstallerSource, InstallConfig, InstallType, LoadedConfig};

pub async fn run(args: &ValidateArgs) -> Result<()> {
    let config_path = resolve_common_config_path(args.config.as_deref());
    let loaded = super::load_config_from(&config_path)?;
    let mut issues: Vec<String> = Vec::new();

    match &loaded {
        LoadedConfig::Standalone { common, specific } => {
            print_standalone_summary(&config_path, common, specific);
            println!("\n[预检查]");
            check_package(&common.installer, true, &mut issues);
            if specific.ssh_target.is_none() {
                check_local_install(&specific.install_path, &mut issues);
            }
            check_standalone_archive(specific, &mut issues);
            check_standalone_ssh(specific, &mut issues).await;
        }
        LoadedConfig::Cluster { install_type, common, specific } => {
            print_cluster_summary(&config_path, *install_type, common, specific);
            println!("\n[预检查]");
            check_package(&common.installer, false, &mut issues);
            check_cluster_ssh(specific, &mut issues).await;
        }
    }

    if issues.is_empty() {
        println!("\n✓ 配置合法，可以执行安装");
        Ok(())
    } else {
        anyhow::bail!("{} 项预检查未通过，请根据上述提示修正配置", issues.len())
    }
}

/// 将用户传入的路径规范化为通用配置（config.toml）路径。
/// 若传入的是特有配置文件（standalone.toml / dw.toml 等），自动切换到同目录的 config.toml。
fn resolve_common_config_path(input: Option<&Path>) -> std::path::PathBuf {
    let path = match input {
        None => return std::path::PathBuf::from(super::CONFIG_FILE),
        Some(p) => p,
    };
    let specific_names = ["standalone.toml", "dw.toml", "rws.toml", "dsc.toml", "dpc.toml"];
    if let Some(name) = path.file_name().and_then(|f| f.to_str())
        && specific_names.contains(&name)
    {
        let dir = path.parent().unwrap_or(Path::new("."));
        let common = dir.join(super::CONFIG_FILE);
        println!("提示: {} 是特有配置文件，自动切换到 {} 进行验证", name, common.display());
        return common;
    }
    path.to_path_buf()
}

fn print_standalone_summary(path: &Path, common: &CommonConfig, cfg: &InstallConfig) {
    println!("配置文件: {} + {}", path.display(), InstallType::Standalone.specific_config_file());
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
    println!("  字符集:     {} ({})", charset_name(cfg.charset), cfg.charset);
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
    let auth = if target.password.is_some() { "密码（已配置）" } else { "密码（安装时将提示输入）" };
    println!("  认证: {}", auth);
}

fn print_cluster_summary(
    path: &Path,
    install_type: InstallType,
    common: &CommonConfig,
    cfg: &ClusterSpecificConfig,
) {
    println!("配置文件: {} + {}", path.display(), install_type.specific_config_file());
    println!("安装类型: {}", install_type_display(install_type));
    println!("\n[集群配置]");
    println!("  OGUID:  {}", cfg.oguid);
    println!("  节点数: {}", cfg.nodes.len());
    match &common.installer {
        InstallerSource::LocalFile(p) => println!("  安装包: {}", p.display()),
        InstallerSource::Url(u) => println!("  安装包: 下载 {}", u),
        InstallerSource::Auto => println!("  安装包: 自动检测下载（假定集群节点与控制机平台一致）"),
    }
    if let Some(storage) = &cfg.shared_storage {
        println!("  共享存储: {}", storage);
    }
    print_dminit_section(&cfg.dminit);
    print_archive_section(&cfg.archive, "各节点 {data_path}/arch（默认）");
    print_mal_section(&cfg.mal);
    print_watcher_section(&cfg.watcher);
    print_dm_ini_section(&cfg.dm_ini);
    print_sqllog_section(&cfg.sqllog);
    println!("\n[节点详情]");
    for node in &cfg.nodes {
        print_node_summary(node, &cfg.dminit);
    }
}

/// `default_path` 是 arch_path 为 None 时显示的描述文本，调用方负责格式化。
fn print_archive_section(arch: &ArchiveConfig, default_path: &str) {
    println!("\n[归档配置]");
    match &arch.arch_path {
        Some(p) => println!("  归档目录:   {}", p),
        None => println!("  归档目录:   {}", default_path),
    }
    println!("  文件大小:   {} MB", arch.file_size);
    if arch.space_limit == 0 {
        println!("  空间上限:   无限制");
    } else {
        println!("  空间上限:   {} MB", arch.space_limit);
    }
    println!("  归档挂起:   {}", yn(arch.hang_flag));
    println!("  归档压缩:   {}", yn(arch.compressed));
}

fn print_mal_section(mal: &crate::config::cluster::MalConfig) {
    println!("\n[MAL 链路]");
    println!("  心跳间隔:   {} 秒", mal.check_interval);
    println!("  重试间隔:   {} 秒", mal.conn_fail_interval);
    println!("  实例缓冲:   {} MB", mal.buf_size);
    println!("  系统缓冲:   {} MB", mal.sys_buf_size);
    println!("  压缩级别:   {}", mal.compress_level);
}

fn print_watcher_section(watcher: &crate::config::cluster::WatcherConfig) {
    use crate::config::cluster::DwMode;
    println!("\n[守护进程]");
    let mode = if watcher.dw_mode == DwMode::Auto { "AUTO（自动故障切换）" } else { "MANUAL（手动）" };
    println!("  守护模式:       {}", mode);
    println!("  守护错误判定:   {} 秒", watcher.dw_error_time);
    println!("  实例恢复等待:   {} 秒", watcher.inst_recover_time);
    println!("  实例错误判定:   {} 秒", watcher.inst_error_time);
    println!("  自动重启:       {}", yn(watcher.inst_auto_restart != 0));
    if watcher.rlog_send_threshold > 0 {
        println!("  redo 发送阈值:  {} 秒", watcher.rlog_send_threshold);
    }
    if watcher.rlog_apply_threshold > 0 {
        println!("  redo 应用阈值:  {} 秒", watcher.rlog_apply_threshold);
    }
    if let Some(cmd) = &watcher.inst_startup_cmd {
        println!("  启动命令:       {}", cmd);
    }
}

fn print_dm_ini_section(dm_ini: &crate::config::cluster::DmIniConfig) {
    println!("\n[dm.ini 集群参数]");
    println!("  ENABLE_OFFLINE_TS: {}", dm_ini.enable_offline_ts);
}

fn print_sqllog_section(sqllog: &crate::config::cluster::SqlLogConfig) {
    println!("\n[SQL 日志]");
    if !sqllog.enabled {
        println!("  已禁用（安装后可通过 SP_SET_PARA_VALUE 手动开启）");
        return;
    }
    println!("  启用:         是（数据库 open 后通过 SQL 自动配置）");
    println!("  单文件大小:   {} MB", sqllog.file_size);
    println!("  保留文件数:   {}", sqllog.file_num);
    if sqllog.min_exec_time == 0 {
        println!("  执行时间阈值: 不限（记录全部 SQL）");
    } else {
        println!("  执行时间阈值: {} ms", sqllog.min_exec_time);
    }
}

fn print_dminit_section(dminit: &crate::config::cluster::DminitConfig) {
    println!("\n[dminit 参数]");
    println!("  安装路径:   {}", dminit.install_path);
    println!("  数据路径:   {}", dminit.data_path);
    println!("  端口:       {}", dminit.port);
    println!("  页大小:     {} KB", dminit.page_size);
    println!("  字符集:     {} ({})", charset_name(dminit.charset), dminit.charset);
    println!("  区分大小写: {}", yn(dminit.case_sensitive));
    println!("  簇大小:     {}", dminit.extent_size);
}

fn print_node_summary(node: &NodeConfig, dminit: &crate::config::cluster::DminitConfig) {
    println!(
        "  {} {}:{} ({})",
        role_display(node.role), node.host, dminit.port, node.instance_name
    );
    println!("    MAL端口:  {} | 守护端口: {} | DW端口: {}", node.mal_port, node.dw_port, node.inst_dw_port);
    let auth = match (&node.ssh.identity_file, &node.ssh.password) {
        (Some(f), _) => format!("密钥 ({})", f.display()),
        (None, Some(_)) => "密码".to_string(),
        (None, None) => "未配置".to_string(),
    };
    println!("    SSH:      {}@{} ({})", node.ssh.user, node.host, auth);
    println!();
}

fn check_package(source: &InstallerSource, _is_standalone: bool, issues: &mut Vec<String>) {
    match source {
        InstallerSource::Auto => println!("  ✓ 安装包: 自动检测下载"),
        InstallerSource::LocalFile(p) if p.exists() => println!("  ✓ 安装包路径存在: {}", p.display()),
        InstallerSource::LocalFile(p) => {
            println!("  ✗ 安装包路径不存在: {}", p.display());
            println!("    建议: 检查 installer_package 路径是否正确");
            issues.push(format!("安装包不存在: {}", p.display()));
        }
        InstallerSource::Url(url) => println!("  ✓ 安装包: 将从 {} 下载", url),
    }
}

fn check_local_install(install_path: &str, issues: &mut Vec<String>) {
    let dmserver = Path::new(install_path).join("bin/dmserver");
    if dmserver.exists() {
        println!("  ✗ 已检测到达梦安装: {}", dmserver.display());
        println!("    建议: 如需重新安装请先卸载，或修改 install_path 使用其他目录");
        issues.push(format!("install_path 已有达梦安装: {}", install_path));
    } else {
        println!("  ✓ 安装路径未检测到现有达梦实例");
    }
}

fn check_standalone_archive(cfg: &InstallConfig, issues: &mut Vec<String>) {
    let arch = &cfg.archive;

    if arch.file_size == 0 {
        println!("  ✗ 归档文件大小为 0，archive.file_size 必须 > 0（建议 128）");
        issues.push("archive.file_size 无效: 0".to_string());
    } else {
        println!("  ✓ 归档文件大小: {} MB", arch.file_size);
    }

    if let Some(path) = &arch.arch_path {
        if path.is_empty() {
            println!("  ✗ archive.arch_path 不能为空字符串，删除该行将使用默认路径");
            issues.push("archive.arch_path 为空字符串".to_string());
        } else if !Path::new(path).is_absolute() {
            println!("  ✗ archive.arch_path 必须是绝对路径: {}", path);
            issues.push(format!("archive.arch_path 非绝对路径: {}", path));
        } else {
            println!("  ✓ 归档目录: {}", path);
        }
    } else {
        println!("  ✓ 归档目录: {}/arch（默认）", cfg.data_path);
    }
}

async fn check_standalone_ssh(specific: &InstallConfig, issues: &mut Vec<String>) {
    let Some(target) = &specific.ssh_target else { return; };
    if target.password.is_none() {
        println!(
            "  ~ SSH 目标 {}@{}:{}: 密码未配置，安装时将提示输入，跳过连通性检查",
            target.user, target.host, target.ssh_port
        );
        return;
    }
    let creds = SshCredentials { user: target.user.clone(), identity_file: None, password: target.password.clone(), port: target.ssh_port };
    match SshSession::connect(&target.host, target.ssh_port, &creds).await {
        Ok(session) => {
            println!("  ✓ SSH 目标可连通: {}@{}:{}", target.user, target.host, target.ssh_port);
            check_remote_install(&specific.install_path, &session, issues).await;
        }
        Err(e) => {
            println!("  ✗ SSH 目标无法连接 {}@{}:{}: {e}", target.user, target.host, target.ssh_port);
            println!("    建议: 检查 ssh_target.host、ssh_port 和 password 配置");
            issues.push(format!("SSH 无法连接: {}:{}", target.host, target.ssh_port));
        }
    }
}

async fn check_cluster_ssh(specific: &ClusterSpecificConfig, issues: &mut Vec<String>) {
    for node in &specific.nodes {
        match SshSession::connect(&node.host, 22, &node.ssh).await {
            Ok(session) => {
                println!("  ✓ 节点 {} ({}) SSH 可连通", node.host, node.instance_name);
                check_remote_install(&specific.dminit.install_path, &session, issues).await;
            }
            Err(e) => {
                println!("  ✗ 节点 {} ({}) SSH 无法连接: {e}", node.host, node.instance_name);
                println!("    建议: 检查节点 host 配置和 SSH 凭据 (identity_file 或 password)");
                issues.push(format!("节点 {} SSH 无法连接", node.host));
            }
        }
    }
}

async fn check_remote_install(install_path: &str, session: &dyn CommandRunner, issues: &mut Vec<String>) {
    let cmd = format!("test -f '{}/bin/dmserver' && echo exists || echo absent", install_path);
    match session.exec(&cmd).await {
        Ok((out, _)) if String::from_utf8_lossy(&out).trim() == "exists" => {
            println!("    ✗ 已检测到达梦安装: {}/bin/dmserver", install_path);
            println!("      建议: 如需重新安装请先卸载，或修改 install_path");
            issues.push(format!("install_path 已有达梦安装: {}", install_path));
        }
        Ok(_) => println!("    ✓ 安装路径未检测到现有达梦实例"),
        Err(e) => println!("    ~ 安装状态检测失败: {e}（跳过）"),
    }
}

fn charset_name(charset: u8) -> &'static str {
    match charset { 0 => "GB18030", 1 => "UTF-8", 2 => "EUC-KR", _ => "未知" }
}

fn yn(b: bool) -> &'static str {
    if b { "是" } else { "否" }
}

fn install_type_display(t: InstallType) -> &'static str {
    match t {
        InstallType::Standalone => "单机 (standalone)",
        InstallType::Dw => "主备集群 (DW)",
        InstallType::Rws => "读写分离集群 (RWS)",
        InstallType::Dsc => "DSC 共享存储集群",
        InstallType::Dpc => "DPC 分布式集群",
    }
}

fn role_display(role: NodeRole) -> &'static str {
    match role {
        NodeRole::Primary => "主节点 (primary)",
        NodeRole::Standby => "备节点 (standby)",
        NodeRole::Monitor => "监控节点 (monitor)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_check_package_auto_download_standalone() {
        let mut issues = Vec::new();
        check_package(&InstallerSource::Auto, true, &mut issues);
        assert!(issues.is_empty(), "单机 Auto 不应报错");
    }

    #[test]
    fn test_check_package_auto_cluster() {
        let mut issues = Vec::new();
        check_package(&InstallerSource::Auto, false, &mut issues);
        assert!(issues.is_empty(), "集群 Auto 应自动检测，不报错");
    }

    #[test]
    fn test_check_package_nonexistent_path() {
        let mut issues = Vec::new();
        check_package(&InstallerSource::LocalFile(Path::new("/nonexistent/dm.iso").to_path_buf()), true, &mut issues);
        assert_eq!(issues.len(), 1, "路径不存在应报错");
        assert!(issues[0].contains("不存在"), "错误信息应提及不存在");
    }

    #[test]
    fn test_check_package_existing_path() {
        let file = NamedTempFile::new().unwrap();
        let mut issues = Vec::new();
        check_package(&InstallerSource::LocalFile(file.path().to_path_buf()), true, &mut issues);
        assert!(issues.is_empty(), "路径存在应不报错");
    }

    #[test]
    fn test_check_package_url_always_ok() {
        let mut issues = Vec::new();
        check_package(&InstallerSource::Url("https://example.com/dm8.zip".to_string()), false, &mut issues);
        assert!(issues.is_empty(), "URL 来源不应报错");
    }

    #[test]
    fn test_check_local_install_no_dmserver() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut issues = Vec::new();
        check_local_install(dir.path().to_str().unwrap(), &mut issues);
        assert!(issues.is_empty(), "无 dmserver 不应报错");
    }

    #[test]
    fn test_check_local_install_detects_dmserver() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        std::fs::write(dir.path().join("bin/dmserver"), b"fake").unwrap();
        let mut issues = Vec::new();
        check_local_install(dir.path().to_str().unwrap(), &mut issues);
        assert_eq!(issues.len(), 1, "已有 dmserver 应报错");
        assert!(issues[0].contains("已有达梦安装"), "应提示已安装");
    }

    #[test]
    fn test_check_standalone_archive_default_passes() {
        let cfg = InstallConfig::default();
        let mut issues = Vec::new();
        check_standalone_archive(&cfg, &mut issues);
        assert!(issues.is_empty(), "默认归档配置应通过验证");
    }

    #[test]
    fn test_check_standalone_archive_zero_file_size_fails() {
        use crate::config::ArchiveConfig;
        let cfg = InstallConfig {
            archive: ArchiveConfig { file_size: 0, ..Default::default() },
            ..Default::default()
        };
        let mut issues = Vec::new();
        check_standalone_archive(&cfg, &mut issues);
        assert_eq!(issues.len(), 1, "file_size=0 应报错");
        assert!(issues[0].contains("file_size"), "错误信息应提及 file_size");
    }

    #[test]
    fn test_check_standalone_archive_relative_path_fails() {
        use crate::config::ArchiveConfig;
        let cfg = InstallConfig {
            archive: ArchiveConfig {
                arch_path: Some("relative/path".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut issues = Vec::new();
        check_standalone_archive(&cfg, &mut issues);
        assert_eq!(issues.len(), 1, "相对路径应报错");
        assert!(issues[0].contains("非绝对路径"), "错误信息应提及非绝对路径");
    }

    #[test]
    fn test_check_standalone_archive_empty_path_fails() {
        use crate::config::ArchiveConfig;
        let cfg = InstallConfig {
            archive: ArchiveConfig {
                arch_path: Some(String::new()),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut issues = Vec::new();
        check_standalone_archive(&cfg, &mut issues);
        assert_eq!(issues.len(), 1, "空字符串路径应报错");
    }

    #[test]
    fn test_check_standalone_archive_absolute_path_passes() {
        use crate::config::ArchiveConfig;
        #[cfg(not(windows))]
        let path = "/opt/dmdbms/arch".to_string();
        #[cfg(windows)]
        let path = r"C:\dmdbms\arch".to_string();
        let cfg = InstallConfig {
            archive: ArchiveConfig {
                arch_path: Some(path),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut issues = Vec::new();
        check_standalone_archive(&cfg, &mut issues);
        assert!(issues.is_empty(), "合法绝对路径应通过验证");
    }

    #[test]
    fn test_charset_name_known() {
        assert_eq!(charset_name(0), "GB18030");
        assert_eq!(charset_name(1), "UTF-8");
        assert_eq!(charset_name(2), "EUC-KR");
    }

    #[test]
    fn test_yn() {
        assert_eq!(yn(true), "是");
        assert_eq!(yn(false), "否");
    }
}
