use anyhow::{bail, Context, Result};
use std::path::Path;

use crate::cli::InstallArgs;
use crate::config::{CommonConfig, InstallConfig};

pub mod checksum;
pub mod checkpoint;
pub mod env_setup;
pub mod init;
pub mod package;
pub mod remote;
pub mod rollback;
pub mod service;
pub mod silent_install;

/// 单机安装入口。common 和 specific 已由调用方从配置文件加载并验证。
pub async fn run(args: &InstallArgs, common: CommonConfig, specific: InstallConfig) -> Result<()> {
    if let Some(target) = &specific.ssh_target {
        if is_local_host(&target.host) {
            tracing::info!("ssh_target.host 为本机，跳过 SSH，执行本地安装");
        } else {
            return remote::run(args, common, &specific, target).await;
        }
    }

    tracing::info!("开始安装达梦数据库（单机）");
    check_local_prerequisites(&specific.install_path, specific.port)?;

    let mut rb = rollback::StandaloneRollback::new(
        &specific.install_path,
        &specific.data_path,
        &specific.instance_name,
    );

    let env_backup = rollback::EnvBackup::capture()?;
    env_setup::run_local()?;
    rb.set_env_backup(env_backup);

    // [1/7] 加载或创建 checkpoint（跨重试持久化密码和各步骤进度）
    let existing_cp = checkpoint::load(&specific.install_path)?;
    let (sysdba_pwd, sysauditor_pwd) = match &existing_cp {
        Some(c) => (c.sysdba_pwd.clone(), c.sysauditor_pwd.clone()),
        None => generate_passwords(),
    };
    let mut cp = existing_cp.unwrap_or_else(|| {
        checkpoint::Checkpoint::new(&specific.install_path, sysdba_pwd.clone(), sysauditor_pwd.clone())
    });
    cp.save()?;

    // [2/7] 获取安装包（自动下载时缓存到 CWD，支持续传）
    let package_path = resolve_package(args, &common.installer, &mut cp).await?;
    verify_checksum(args, &package_path)?;

    // [3/7] 提取 DMInstall.bin
    let extract_dir = step_extract(&package_path)?;

    // [4/7] 静默安装 dmdbms
    let bin_installed = Path::new(&specific.install_path).join("bin/dminit").exists();
    if cp.installed || bin_installed {
        println!("[续] 跳过安装，dmdbms 已安装至 {}", specific.install_path);
    } else {
        step_silent_install(&specific, &extract_dir)?;
        cp.installed = true;
        cp.save()?;
    }
    rb.installed = true;

    // [5/7] dminit 初始化（幂等：dm.ini 存在则跳过）
    let dm_ini_exists = Path::new(&specific.data_path).join("dm.ini").exists();
    if dm_ini_exists {
        println!("[续] 跳过 dminit，实例已初始化");
    } else {
        step_dminit(&specific, &sysdba_pwd, &sysauditor_pwd)?;
        step_write_dmarch_ini(&specific)?;
    }
    rb.db_inited = true;

    // [6/7] 注册并启动 DM 系统服务
    step_register_service(&specific)?;
    rb.services_registered = true;

    // [6c/7] 开启归档模式（数据库已 open 后通过 disql 执行）
    step_enable_archivelog(&specific, &sysdba_pwd)?;

    // [7/7] 打印凭证，清理临时文件
    print_generated_credentials(&sysdba_pwd, &sysauditor_pwd);
    if let Some(cached) = &cp.package_cache {
        let _ = std::fs::remove_file(cached);
    }
    checkpoint::Checkpoint::remove()?;
    tracing::info!("单机安装完成");
    rb.commit();
    Ok(())
}

/// 判断 host 是否指向本机：localhost 别名 + 系统 hostname。
fn is_local_host(host: &str) -> bool {
    if matches!(host, "localhost" | "127.0.0.1" | "::1") {
        return true;
    }
    std::process::Command::new("hostname")
        .output()
        .map(|o| {
            let sys_hostname = String::from_utf8_lossy(&o.stdout);
            sys_hostname.trim() == host
        })
        .unwrap_or(false)
}

fn generate_passwords() -> (String, String) {
    (generate_password(), generate_password())
}

/// 生成满足达梦密码策略的随机密码（16 位，含大写/小写/数字/特殊字符）。
pub(crate) fn generate_password() -> String {
    use rand::Rng;
    const UPPER: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    const LOWER: &[u8] = b"abcdefghjkmnpqrstuvwxyz";
    const DIGITS: &[u8] = b"23456789";
    const SPECIAL: &[u8] = b"@#$%&*!";
    const ALL: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789@#$%&*!";

    let mut rng = rand::thread_rng();
    let mut pwd: Vec<u8> = vec![
        UPPER[rng.gen_range(0..UPPER.len())],
        LOWER[rng.gen_range(0..LOWER.len())],
        DIGITS[rng.gen_range(0..DIGITS.len())],
        SPECIAL[rng.gen_range(0..SPECIAL.len())],
    ];
    for _ in 0..12 {
        pwd.push(ALL[rng.gen_range(0..ALL.len())]);
    }
    use rand::seq::SliceRandom;
    pwd.shuffle(&mut rng);
    String::from_utf8(pwd).expect("charset is ASCII")
}

fn print_generated_credentials(sysdba_pwd: &str, sysauditor_pwd: &str) {
    println!();
    println!("╔══════════════════════════════════════════════════╗");
    println!("║              达梦数据库初始凭证                  ║");
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  SYSDBA    密码: {:<33}║", sysdba_pwd);
    println!("║  SYSAUDITOR密码: {:<33}║", sysauditor_pwd);
    println!("╠══════════════════════════════════════════════════╣");
    println!("║  首次登录后请立即修改密码                        ║");
    println!("╚══════════════════════════════════════════════════╝");
    println!();
}

#[cfg(test)]
fn validate_password_complexity(pwd: &str) -> Result<()> {
    if pwd.len() < 9 {
        anyhow::bail!("密码长度不足 9 位（达梦密码策略要求）");
    }
    let categories = [
        pwd.chars().any(|c| c.is_ascii_uppercase()),
        pwd.chars().any(|c| c.is_ascii_lowercase()),
        pwd.chars().any(|c| c.is_ascii_digit()),
        pwd.chars().any(|c| !c.is_alphanumeric()),
    ];
    if categories.iter().filter(|&&b| b).count() < 3 {
        anyhow::bail!("密码复杂度不足——需含大写字母、小写字母、数字、特殊字符中的至少三类");
    }
    Ok(())
}

/// 解析安装包路径：CLI > config > checkpoint 缓存 > 自动下载。
/// 需要网络下载时将结果持久化到 CWD（.dm_cache_ 前缀），支持中断续传。
async fn resolve_package(
    args: &InstallArgs,
    installer: &crate::config::InstallerSource,
    cp: &mut checkpoint::Checkpoint,
) -> Result<std::path::PathBuf> {
    use crate::config::InstallerSource;
    tracing::info!("[2/7] 获取安装包路径");

    if let Some(p) = &args.package {
        println!("使用本地安装包 (CLI --package): {}", p.display());
        return Ok(p.clone());
    }
    if let Some(url) = &args.url {
        println!("下载安装包 (CLI --url): {}", url);
        let handle = crate::common::download::fetch_from_url(url).await?;
        let cached = cache_package(&handle.path)?;
        cp.package_cache = Some(cached.to_string_lossy().into_owned());
        cp.save()?;
        return Ok(cached);
    }

    match installer {
        InstallerSource::LocalFile(path) => {
            println!("使用本地安装包 (config.toml): {}", path.display());
            Ok(path.clone())
        }
        InstallerSource::Url(url) => {
            if let Some(cached) = cp.package_cache.as_ref().map(std::path::Path::new).filter(|p| p.exists()) {
                println!("[续] 跳过下载，使用已缓存安装包: {}", cached.display());
                return Ok(cached.to_path_buf());
            }
            let handle = crate::common::download::fetch_from_url(url).await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
        InstallerSource::Auto => {
            if let Some(cached) = cp.package_cache.as_ref().map(std::path::Path::new).filter(|p| p.exists()) {
                println!("[续] 跳过下载，使用已缓存安装包: {}", cached.display());
                return Ok(cached.to_path_buf());
            }
            let handle = crate::common::download::fetch_dm_installer().await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
    }
}

/// 将安装包复制到 CWD，文件名加 `.dm_cache_` 前缀，避免与用户文件冲突。
fn cache_package(src: &std::path::Path) -> Result<std::path::PathBuf> {
    let file_name = src
        .file_name()
        .map(|n| format!(".dm_cache_{}", n.to_string_lossy()))
        .unwrap_or_else(|| ".dm_cache_installer".to_string());
    let dest = std::env::current_dir()?.join(file_name);
    if dest.exists() {
        return Ok(dest);
    }
    std::fs::copy(src, &dest)
        .with_context(|| format!("缓存安装包失败: {} -> {}", src.display(), dest.display()))?;
    Ok(dest)
}

fn verify_checksum(args: &InstallArgs, path: &std::path::Path) -> Result<()> {
    tracing::info!("[3/7] SHA-256 校验");
    if let Some(expected) = &args.checksum {
        checksum::verify_sha256(path, expected)
    } else {
        tracing::warn!("未提供 --checksum，跳过 SHA-256 校验");
        Ok(())
    }
}

fn step_extract(path: &std::path::Path) -> Result<tempfile::TempDir> {
    tracing::info!("[4/7] 提取 DMInstall.bin");
    package::extract_dminstall_bin(path)
}

fn step_silent_install(config: &InstallConfig, extract_dir: &tempfile::TempDir) -> Result<()> {
    tracing::info!("[5/7] 安装 dmdbms");
    let bin_path = extract_dir.path().join("DMInstall.bin");
    silent_install::install_from_bin(&bin_path, &config.install_path)
}

fn step_dminit(config: &InstallConfig, sysdba_pwd: &str, sysauditor_pwd: &str) -> Result<()> {
    tracing::info!("[6/7] dminit 初始化");
    init::run_dminit(config, sysdba_pwd, sysauditor_pwd)
}

fn step_write_dmarch_ini(config: &InstallConfig) -> Result<()> {
    tracing::info!("[6b/7] 写入 dmarch.ini");
    init::write_dmarch_ini(config)
}

fn step_register_service(config: &InstallConfig) -> Result<()> {
    tracing::info!("[7/7] 注册并启动 DM 服务");
    service::register_and_start(config)
}

fn step_enable_archivelog(config: &InstallConfig, sysdba_pwd: &str) -> Result<()> {
    tracing::info!("[7b/7] 开启归档模式");
    init::enable_archivelog(config, sysdba_pwd)
}

fn check_local_prerequisites(install_path: &str, port: u16) -> Result<()> {
    tracing::info!("[预检查] 本地硬件资源检测");
    check_local_port(port)?;
    check_local_disk(install_path)?;
    check_local_memory()?;
    check_local_cpu()?;
    check_local_ulimits();
    check_local_selinux();
    Ok(())
}

fn check_local_port(port: u16) -> Result<()> {
    let output = std::process::Command::new("ss")
        .args(["-tlnp"])
        .output()
        .context("执行 ss 失败，请确认 iproute2 已安装")?;
    let text = std::str::from_utf8(&output.stdout).unwrap_or("");
    let port_str = port.to_string();
    let occupied = text.lines().any(|line| {
        if let Some(idx) = line.find(&format!(":{port_str}")) {
            let after = &line[idx + 1 + port_str.len()..];
            !after.starts_with(|c: char| c.is_ascii_digit())
        } else {
            false
        }
    });
    if occupied {
        bail!("[预检查] 端口 {} 已被占用，请修改配置中的 port 参数或释放该端口", port);
    }
    Ok(())
}

fn check_local_ulimits() {
    let content = match std::fs::read_to_string("/proc/self/limits") {
        Ok(c) => c,
        Err(_) => return,
    };
    const MIN: u64 = 65536;
    for line in content.lines() {
        let (name, col) = if line.starts_with("Max open files") {
            ("nofile", 3usize)
        } else if line.starts_with("Max processes") {
            ("nproc", 2usize)
        } else {
            continue;
        };
        let soft: u64 = match line.split_whitespace().nth(col).and_then(|s| s.parse().ok()) {
            Some(n) => n,
            None => continue, // "unlimited" 解析失败 → 视为无限制
        };
        if soft < MIN {
            tracing::warn!(
                "[预检查] {} soft limit = {}，建议 >= {}；\
                 请在 /etc/security/limits.conf 中添加: dmdba soft {} {}",
                name, soft, MIN, name, MIN
            );
        }
    }
}

fn check_local_selinux() {
    let output = match std::process::Command::new("getenforce").output() {
        Ok(o) => o,
        Err(_) => return,
    };
    let mode = std::str::from_utf8(&output.stdout).unwrap_or("").trim();
    if mode == "Enforcing" {
        tracing::warn!(
            "[预检查] SELinux 处于 Enforcing 模式，可能阻断 DM 进程启动；\
             临时切换: setenforce 0；\
             永久禁用: 将 /etc/selinux/config 中 SELINUX=enforcing 改为 permissive 并重启"
        );
    }
}

fn check_local_disk(install_path: &str) -> Result<()> {
    let parent = Path::new(install_path)
        .parent()
        .unwrap_or_else(|| Path::new("/"));
    let output = std::process::Command::new("df")
        .arg("-B1")
        .arg(parent)
        .output()
        .context("执行 df 失败")?;
    let available = parse_df_bytes(&output.stdout)?;
    let min_bytes: u64 = 20 * 1024 * 1024 * 1024;
    tracing::debug!(
        "[预检查] 磁盘剩余: {} GB，最低要求: 20 GB",
        available / (1024 * 1024 * 1024)
    );
    if available < min_bytes {
        bail!("[预检查] 磁盘空间不足: 剩余 {} bytes，需要 >= 20 GB", available);
    }
    Ok(())
}

fn check_local_memory() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let content = std::fs::read_to_string("/proc/meminfo")
            .context("无法读取 /proc/meminfo")?;
        let line = content
            .lines()
            .find(|l| l.starts_with("MemTotal:"))
            .context("未找到 MemTotal 行")?;
        let total_kb: u64 = line
            .split_whitespace()
            .nth(1)
            .context("MemTotal 行格式异常")?
            .parse()
            .context("MemTotal 值无法解析")?;
        let min_kb: u64 = 4 * 1024 * 1024;
        tracing::debug!(
            "[预检查] 内存总量: {} GB，最低要求: 4 GB",
            total_kb / (1024 * 1024)
        );
        if total_kb < min_kb {
            bail!("[预检查] 内存不足: {} KB，需要 >= 4 GB", total_kb);
        }
    }
    Ok(())
}

fn check_local_cpu() -> Result<()> {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    if cores < 1 {
        bail!("[预检查] CPU 核心数不足: {} 核，需要 >= 1 核", cores);
    }
    Ok(())
}

fn parse_df_bytes(stdout: &[u8]) -> Result<u64> {
    let text = std::str::from_utf8(stdout).context("df 输出不是有效 UTF-8")?;
    let second_line = text.lines().nth(1).context("df 输出行数不足")?;
    let available_str = second_line
        .split_whitespace()
        .nth(3)
        .context("df 输出列数不足")?;
    available_str
        .parse::<u64>()
        .context(format!("df Available 列无法解析: {available_str}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_complexity_rejects_short() {
        assert!(validate_password_complexity("Ab1!").is_err());
    }

    #[test]
    fn test_password_complexity_rejects_simple() {
        assert!(validate_password_complexity("password123").is_err());
    }

    #[test]
    fn test_password_complexity_accepts_valid() {
        assert!(validate_password_complexity("DMAdmin1@2024").is_ok());
    }

    #[test]
    fn test_generate_password_length() {
        let pwd = generate_password();
        assert_eq!(pwd.len(), 16, "生成的密码应为 16 位");
    }

    #[test]
    fn test_generate_password_meets_complexity() {
        for _ in 0..20 {
            let pwd = generate_password();
            assert!(validate_password_complexity(&pwd).is_ok(), "生成的密码应满足复杂度: {}", pwd);
        }
    }

    #[test]
    fn test_generate_password_is_ascii() {
        let pwd = generate_password();
        assert!(pwd.is_ascii(), "生成的密码应为纯 ASCII: {}", pwd);
    }
}
