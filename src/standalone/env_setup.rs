use anyhow::{Context, Result};

use crate::common::ssh::CommandRunner;

// ── 公共入口 ──────────────────────────────────────────────────────────────────

/// 本地安装前环境准备。失败则阻断安装。
pub fn run_local() -> Result<()> {
    println!("[环境准备] 检测执行权限...");
    let use_sudo = detect_local_privilege()?;

    println!("[环境准备] 配置系统内核参数和用户权限...");
    setup_dmdba_user(use_sudo)?;
    disable_selinux(use_sudo)?;
    disable_transparent_hugepages(use_sudo)?;
    set_timezone(use_sudo)?;
    set_locale(use_sudo)?;
    optimize_sshd(use_sudo)?;
    disable_firewall(use_sudo)?;
    configure_limits(use_sudo)?;
    configure_pam(use_sudo)?;
    configure_sysctl(use_sudo)?;

    println!("[环境准备] 完成");
    Ok(())
}

/// 远端（SSH）安装前环境准备。
pub async fn run_remote(runner: &dyn CommandRunner) -> Result<()> {
    println!("[环境准备] 检测远端执行权限...");
    remote_check_privilege(runner).await?;

    println!("[环境准备] 配置远端系统内核参数和用户权限...");
    remote_setup_dmdba_user(runner).await?;
    remote_disable_selinux(runner).await?;
    remote_disable_thp(runner).await?;
    remote_set_timezone(runner).await?;
    remote_set_locale(runner).await?;
    remote_optimize_sshd(runner).await?;
    remote_disable_firewall(runner).await?;
    remote_configure_limits(runner).await?;
    remote_configure_pam(runner).await?;
    remote_configure_sysctl(runner).await?;

    println!("[环境准备] 完成");
    Ok(())
}

// ── 权限检测 ──────────────────────────────────────────────────────────────────

/// 检测本地是否拥有足够权限（root 或免密 sudo）。
/// 返回 true 表示需要 sudo 前缀，false 表示已是 root。
fn detect_local_privilege() -> Result<bool> {
    if is_root() {
        tracing::debug!("当前以 root 运行");
        return Ok(false);
    }
    if can_sudo() {
        tracing::info!("当前为非 root 用户，将使用 sudo 执行特权操作");
        return Ok(true);
    }
    anyhow::bail!(
        "环境准备需要 root 权限，当前用户 ({}) 既非 root 也无免密 sudo。\n\
         请以 root 身份重新运行，或执行以下命令为当前用户授权：\n\
         echo '$USER ALL=(ALL) NOPASSWD:ALL' | sudo tee /etc/sudoers.d/$USER",
        std::env::var("USER").unwrap_or_else(|_| "unknown".into())
    )
}

fn is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
        .unwrap_or(false)
}

fn can_sudo() -> bool {
    std::process::Command::new("sudo")
        .args(["-n", "true"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn remote_check_privilege(runner: &dyn CommandRunner) -> Result<()> {
    let uid = remote_read_str(runner, "id -u").await;
    if uid.trim() == "0" {
        return Ok(());
    }
    let sudo_ok = remote_read_str(runner, "sudo -n true 2>/dev/null && echo ok || echo fail").await;
    if sudo_ok.trim() == "ok" {
        tracing::info!("远端为非 root 用户，将使用 sudo 执行特权操作");
        return Ok(());
    }
    anyhow::bail!(
        "远端环境准备需要 root 权限，当前远端 UID={}，且无免密 sudo。\n\
         请在 config 的 ssh_target 中配置 root 用户，或为 SSH 用户授权免密 sudo。",
        uid.trim()
    )
}

// ── 本地步骤：dmdba 用户 ──────────────────────────────────────────────────────

fn setup_dmdba_user(use_sudo: bool) -> Result<()> {
    if !cmd_succeeds("getent", &["group", "dinstall"]) {
        run_priv(use_sudo, "groupadd", &["-g", "1002", "dinstall"], "创建用户组 dinstall 失败")?;
    }
    if !cmd_succeeds("id", &["dmdba"]) {
        run_priv(
            use_sudo, "useradd",
            &["-u", "1002", "-g", "dinstall", "-m", "-d", "/home/dmdba", "-s", "/bin/bash", "dmdba"],
            "创建用户 dmdba 失败",
        )?;
    }
    pipe_to_priv(use_sudo, b"dmdba:dmdba", "chpasswd", &[], "设置 dmdba 密码失败")?;
    run_priv(use_sudo, "chage", &["-M", "-1", "dmdba"], "设置密码永不过期失败")?;

    // 验证
    if !cmd_succeeds("id", &["dmdba"]) {
        anyhow::bail!("用户 dmdba 创建后验证失败，请检查 useradd 输出");
    }
    tracing::info!("dmdba 用户已就绪");
    Ok(())
}

// ── 本地步骤：SELinux ─────────────────────────────────────────────────────────

fn disable_selinux(use_sudo: bool) -> Result<()> {
    // 运行时切换（允许失败，如 SELinux 未安装）
    let _ = run_priv(use_sudo, "setenforce", &["0"], "");

    let cfg = "/etc/selinux/config";
    if std::path::Path::new(cfg).exists() {
        run_priv(
            use_sudo, "sed",
            &["-i", "/^SELINUX=/cSELINUX=disabled", cfg],
            "修改 /etc/selinux/config 失败",
        )?;
        // 验证文件内容
        let content = std::fs::read_to_string(cfg).unwrap_or_default();
        if !content.lines().any(|l| l.trim() == "SELINUX=disabled") {
            anyhow::bail!("/etc/selinux/config 中 SELINUX 未成功设置为 disabled");
        }
    }
    // 验证运行时状态不再为 Enforcing
    if let Ok(out) = std::process::Command::new("getenforce").output() {
        let mode = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if mode == "Enforcing" {
            anyhow::bail!("SELinux 运行时仍为 Enforcing，setenforce 0 未生效");
        }
        tracing::info!("SELinux 状态: {mode}");
    }
    Ok(())
}

// ── 本地步骤：Transparent Hugepages ──────────────────────────────────────────

fn disable_transparent_hugepages(use_sudo: bool) -> Result<()> {
    let thp = "/sys/kernel/mm/transparent_hugepage/enabled";
    if std::path::Path::new(thp).exists() {
        // 用 tee 写入（兼容 sudo 场景）
        pipe_to_priv(use_sudo, b"never\n", "tee", &[thp], "关闭 THP 失败")?;
        // 验证
        let val = std::fs::read_to_string(thp).unwrap_or_default();
        if !val.contains("never") {
            anyhow::bail!("Transparent Hugepages 关闭失败，当前值: {}", val.trim());
        }
    }
    // 持久化写入 rc.local
    let rc = "/etc/rc.local";
    if std::path::Path::new(rc).exists() {
        let content = std::fs::read_to_string(rc).unwrap_or_default();
        if !content.contains("transparent_hugepage") {
            append_priv(use_sudo, rc,
                "echo never > /sys/kernel/mm/transparent_hugepage/enabled\n",
                "写入 rc.local 失败")?;
            run_priv(use_sudo, "chmod", &["+x", rc], "chmod rc.local 失败")?;
        }
    }
    tracing::info!("Transparent Hugepages 已关闭");
    Ok(())
}

// ── 本地步骤：时区 ────────────────────────────────────────────────────────────

fn set_timezone(use_sudo: bool) -> Result<()> {
    run_priv(use_sudo, "timedatectl", &["set-timezone", "Asia/Shanghai"], "设置时区失败")?;
    // 验证
    let out = std::process::Command::new("timedatectl")
        .args(["show", "--property=Timezone"])
        .output()
        .context("验证时区失败")?;
    let val = String::from_utf8_lossy(&out.stdout);
    if !val.contains("Asia/Shanghai") {
        anyhow::bail!("时区设置验证失败，当前值: {}", val.trim());
    }
    tracing::info!("时区已设置为 Asia/Shanghai");
    Ok(())
}

// ── 本地步骤：字符集 ──────────────────────────────────────────────────────────

fn set_locale(use_sudo: bool) -> Result<()> {
    let marker = "export LANG=zh_CN.UTF-8";
    let content = std::fs::read_to_string("/etc/profile").unwrap_or_default();
    if !content.contains(marker) {
        append_priv(use_sudo, "/etc/profile", &format!("{marker}\n"), "写入 /etc/profile 失败")?;
        // 验证
        let refreshed = std::fs::read_to_string("/etc/profile").unwrap_or_default();
        if !refreshed.contains(marker) {
            anyhow::bail!("/etc/profile 中 LANG 设置未写入成功");
        }
    }
    tracing::info!("字符集已设置为 zh_CN.UTF-8");
    Ok(())
}

// ── 本地步骤：SSH 优化 ────────────────────────────────────────────────────────

fn optimize_sshd(use_sudo: bool) -> Result<()> {
    let cfg = "/etc/ssh/sshd_config";
    if !std::path::Path::new(cfg).exists() {
        return Ok(());
    }
    for (pattern, replacement) in [
        ("/^#GSSAPIAuthentication/cGSSAPIAuthentication no", ""),
        ("/^GSSAPIAuthentication/cGSSAPIAuthentication no", ""),
        ("/^#UseDNS/cUseDNS no", ""),
        ("/^UseDNS/cUseDNS no", ""),
    ] {
        run_priv(use_sudo, "sed", &["-i", pattern, cfg], "修改 sshd_config 失败")?;
        let _ = replacement; // pattern 直接包含替换内容
    }
    // reload 不断开当前连接；失败则忽略（容器环境可能无 sshd）
    let _ = run_priv(use_sudo, "systemctl", &["reload", "sshd"], "");
    tracing::info!("SSH 配置已优化（GSSAPIAuthentication=no, UseDNS=no）");
    Ok(())
}

// ── 本地步骤：防火墙 ──────────────────────────────────────────────────────────

fn disable_firewall(use_sudo: bool) -> Result<()> {
    // 允许失败（防火墙可能未安装）
    let _ = run_priv(use_sudo, "systemctl", &["stop",    "firewalld"], "");
    let _ = run_priv(use_sudo, "systemctl", &["disable", "firewalld"], "");

    // 验证：active 状态应为 inactive 或 dead
    let out = std::process::Command::new("systemctl")
        .args(["is-active", "firewalld"])
        .output();
    if let Ok(o) = out {
        let state = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if state == "active" {
            anyhow::bail!("防火墙关闭失败，当前状态仍为 active");
        }
    }
    tracing::info!("防火墙已关闭");
    Ok(())
}

// ── 本地步骤：limits.conf ─────────────────────────────────────────────────────

fn configure_limits(use_sudo: bool) -> Result<()> {
    let path = "/etc/security/limits.conf";
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("读取 {path} 失败"))?;
    let mut to_add: Vec<&str> = Vec::new();
    for &line in LIMITS_LINES {
        if !content.contains(line) {
            to_add.push(line);
        }
    }
    if !to_add.is_empty() {
        let addition = to_add.join("\n") + "\n";
        append_priv(use_sudo, path, &addition, "写入 limits.conf 失败")?;
    }
    // 验证
    let updated = std::fs::read_to_string(path).unwrap_or_default();
    for &line in LIMITS_LINES {
        if !updated.contains(line) {
            anyhow::bail!("limits.conf 验证失败，缺少行: {line}");
        }
    }
    tracing::info!("limits.conf 已配置");
    Ok(())
}

// ── 本地步骤：PAM ─────────────────────────────────────────────────────────────

fn configure_pam(use_sudo: bool) -> Result<()> {
    let path = "/etc/pam.d/login";
    if !std::path::Path::new(path).exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(path).unwrap_or_default();
    if content.contains("pam_limits.so") {
        return Ok(());
    }
    let addition = "\nsession    required        /lib64/security/pam_limits.so\nsession    required        pam_limits.so\n";
    append_priv(use_sudo, path, addition, "写入 /etc/pam.d/login 失败")?;
    // 验证
    let updated = std::fs::read_to_string(path).unwrap_or_default();
    if !updated.contains("pam_limits.so") {
        anyhow::bail!("/etc/pam.d/login 中 pam_limits.so 写入验证失败");
    }
    tracing::info!("PAM limits 已配置");
    Ok(())
}

// ── 本地步骤：sysctl.conf ─────────────────────────────────────────────────────

fn configure_sysctl(use_sudo: bool) -> Result<()> {
    let path = "/etc/sysctl.conf";
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let mut to_add: Vec<String> = Vec::new();
    for &(key, line) in SYSCTL_PARAMS {
        if !content.lines().any(|l| l.trim_start().starts_with(key)) {
            to_add.push(line.to_string());
        }
    }
    if !content.lines().any(|l| l.trim_start().starts_with("kernel.shmall")) {
        let (shmall, shmmax) = compute_shm()?;
        to_add.push(format!("kernel.shmall={shmall}"));
        to_add.push(format!("kernel.shmmax={shmmax}"));
    }
    if !to_add.is_empty() {
        let addition = to_add.join("\n") + "\n";
        append_priv(use_sudo, path, &addition, "写入 sysctl.conf 失败")?;
    }
    // 应用并验证
    run_priv(use_sudo, "sysctl", &["-p"], "sysctl -p 执行失败")?;
    verify_sysctl_applied()?;
    tracing::info!("sysctl 内核参数已生效");
    Ok(())
}

fn verify_sysctl_applied() -> Result<()> {
    let out = std::process::Command::new("sysctl")
        .arg("fs.file-max")
        .output()
        .context("验证 sysctl fs.file-max 失败")?;
    let val = String::from_utf8_lossy(&out.stdout);
    if !val.contains("6815744") {
        anyhow::bail!("sysctl fs.file-max 未生效，当前输出: {}", val.trim());
    }
    Ok(())
}

// ── 远端步骤 ──────────────────────────────────────────────────────────────────

async fn remote_setup_dmdba_user(runner: &dyn CommandRunner) -> Result<()> {
    exec_remote(runner,
        "getent group dinstall >/dev/null 2>&1 || groupadd -g 1002 dinstall",
        "创建用户组 dinstall 失败").await?;
    exec_remote(runner,
        "id dmdba >/dev/null 2>&1 || useradd -u 1002 -g dinstall -m -d /home/dmdba -s /bin/bash dmdba",
        "创建用户 dmdba 失败").await?;
    exec_remote(runner, "echo 'dmdba:dmdba' | chpasswd", "设置 dmdba 密码失败").await?;
    exec_remote(runner, "chage -M -1 dmdba", "设置密码永不过期失败").await?;
    // 验证
    let uid_out = remote_read_str(runner, "id -u dmdba 2>/dev/null || echo fail").await;
    if uid_out.trim() == "fail" {
        anyhow::bail!("远端 dmdba 用户创建后验证失败");
    }
    Ok(())
}

async fn remote_disable_selinux(runner: &dyn CommandRunner) -> Result<()> {
    exec_remote(runner,
        "setenforce 0 2>/dev/null || true; \
         [ -f /etc/selinux/config ] && sed -i '/^SELINUX=/cSELINUX=disabled' /etc/selinux/config || true",
        "禁用 SELinux 失败").await?;
    // 验证运行时状态
    let mode = remote_read_str(runner, "getenforce 2>/dev/null || echo Disabled").await;
    if mode.trim() == "Enforcing" {
        anyhow::bail!("远端 SELinux 运行时仍为 Enforcing，setenforce 0 未生效");
    }
    Ok(())
}

async fn remote_disable_thp(runner: &dyn CommandRunner) -> Result<()> {
    exec_remote(runner,
        "[ -f /sys/kernel/mm/transparent_hugepage/enabled ] \
         && echo never | tee /sys/kernel/mm/transparent_hugepage/enabled >/dev/null || true; \
         if [ -f /etc/rc.local ]; then \
           grep -q 'transparent_hugepage' /etc/rc.local 2>/dev/null \
           || echo 'echo never > /sys/kernel/mm/transparent_hugepage/enabled' >> /etc/rc.local; \
           chmod +x /etc/rc.local; \
         fi",
        "关闭 Transparent Hugepages 失败").await?;
    // 验证
    let val = remote_read_str(runner,
        "cat /sys/kernel/mm/transparent_hugepage/enabled 2>/dev/null || echo ok").await;
    if !val.is_empty() && !val.contains("never") && val.trim() != "ok" {
        anyhow::bail!("远端 THP 关闭失败，当前值: {}", val.trim());
    }
    Ok(())
}

async fn remote_set_timezone(runner: &dyn CommandRunner) -> Result<()> {
    exec_remote(runner, "timedatectl set-timezone Asia/Shanghai 2>/dev/null || true", "设置时区失败").await?;
    let val = remote_read_str(runner, "timedatectl show --property=Timezone 2>/dev/null || echo ''").await;
    if !val.is_empty() && !val.contains("Asia/Shanghai") {
        anyhow::bail!("远端时区设置验证失败，当前值: {}", val.trim());
    }
    Ok(())
}

async fn remote_set_locale(runner: &dyn CommandRunner) -> Result<()> {
    exec_remote(runner,
        "grep -q 'export LANG=zh_CN.UTF-8' /etc/profile 2>/dev/null \
         || echo 'export LANG=zh_CN.UTF-8' >> /etc/profile",
        "设置字符集失败").await?;
    let val = remote_read_str(runner, "grep 'LANG=zh_CN.UTF-8' /etc/profile 2>/dev/null || echo ''").await;
    if val.trim().is_empty() {
        anyhow::bail!("远端 /etc/profile 中 LANG 设置写入验证失败");
    }
    Ok(())
}

async fn remote_optimize_sshd(runner: &dyn CommandRunner) -> Result<()> {
    exec_remote(runner,
        "if [ -f /etc/ssh/sshd_config ]; then \
           sed -i '/^#GSSAPIAuthentication/cGSSAPIAuthentication no' /etc/ssh/sshd_config; \
           sed -i '/^GSSAPIAuthentication/cGSSAPIAuthentication no'  /etc/ssh/sshd_config; \
           sed -i '/^#UseDNS/cUseDNS no'                            /etc/ssh/sshd_config; \
           sed -i '/^UseDNS/cUseDNS no'                             /etc/ssh/sshd_config; \
           systemctl reload sshd 2>/dev/null || true; \
         fi",
        "优化 sshd 配置失败").await
}

async fn remote_disable_firewall(runner: &dyn CommandRunner) -> Result<()> {
    exec_remote(runner,
        "systemctl stop firewalld 2>/dev/null || true; \
         systemctl disable firewalld 2>/dev/null || true",
        "关闭防火墙失败").await?;
    let state = remote_read_str(runner,
        "systemctl is-active firewalld 2>/dev/null || echo inactive").await;
    if state.trim() == "active" {
        anyhow::bail!("远端防火墙关闭失败，当前状态仍为 active");
    }
    Ok(())
}

async fn remote_configure_limits(runner: &dyn CommandRunner) -> Result<()> {
    for &line in LIMITS_LINES {
        let cmd = format!(
            "grep -qF '{line}' /etc/security/limits.conf 2>/dev/null \
             || echo '{line}' >> /etc/security/limits.conf"
        );
        exec_remote(runner, &cmd, "配置 limits.conf 失败").await?;
    }
    // 抽查最后一行是否写入
    let last = LIMITS_LINES.last().unwrap();
    let check = remote_read_str(runner,
        &format!("grep -cF '{last}' /etc/security/limits.conf 2>/dev/null || echo 0")).await;
    if check.trim() == "0" {
        anyhow::bail!("远端 limits.conf 写入验证失败");
    }
    Ok(())
}

async fn remote_configure_pam(runner: &dyn CommandRunner) -> Result<()> {
    exec_remote(runner,
        r#"if [ -f /etc/pam.d/login ] && ! grep -q 'pam_limits.so' /etc/pam.d/login 2>/dev/null; then
  printf '\nsession    required        /lib64/security/pam_limits.so\nsession    required        pam_limits.so\n' >> /etc/pam.d/login
fi"#,
        "配置 PAM limits 失败").await?;
    let val = remote_read_str(runner,
        "grep -c 'pam_limits.so' /etc/pam.d/login 2>/dev/null || echo 0").await;
    if val.trim() == "0" {
        anyhow::bail!("远端 /etc/pam.d/login 中 pam_limits.so 写入验证失败");
    }
    Ok(())
}

async fn remote_configure_sysctl(runner: &dyn CommandRunner) -> Result<()> {
    for &(key, line) in SYSCTL_PARAMS {
        let cmd = format!(
            "grep -q '^{key}' /etc/sysctl.conf 2>/dev/null || echo '{line}' >> /etc/sysctl.conf"
        );
        exec_remote(runner, &cmd, "配置 sysctl.conf 失败").await?;
    }
    exec_remote(runner,
        r#"if ! grep -q '^kernel.shmall' /etc/sysctl.conf 2>/dev/null; then
  mem_kb=$(awk '/^MemTotal:/{print $2}' /proc/meminfo)
  shmall=$(awk -v m="$mem_kb" 'BEGIN{printf "%.0f", m*0.64/4}')
  shmmax=$(awk -v m="$mem_kb" 'BEGIN{printf "%.0f", m*0.64*1024}')
  echo "kernel.shmall=$shmall" >> /etc/sysctl.conf
  echo "kernel.shmmax=$shmmax" >> /etc/sysctl.conf
fi"#,
        "配置 kernel.shmall/shmmax 失败").await?;
    exec_remote(runner, "sysctl -p >/dev/null 2>&1 || true", "应用 sysctl 参数失败").await?;
    // 验证
    let val = remote_read_str(runner, "sysctl fs.file-max 2>/dev/null || echo ''").await;
    if !val.is_empty() && !val.contains("6815744") {
        anyhow::bail!("远端 sysctl fs.file-max 未生效，当前: {}", val.trim());
    }
    Ok(())
}

// ── 常量 ──────────────────────────────────────────────────────────────────────

const LIMITS_LINES: &[&str] = &[
    "dmdba  soft  nice     0",
    "dmdba  hard  nice     0",
    "dmdba  soft  as       unlimited",
    "dmdba  hard  as       unlimited",
    "dmdba  soft  fsize    unlimited",
    "dmdba  hard  fsize    unlimited",
    "dmdba  soft  nproc    65536",
    "dmdba  hard  nproc    65536",
    "dmdba  soft  nofile   65536",
    "dmdba  hard  nofile   65536",
    "dmdba  soft  core     unlimited",
    "dmdba  hard  core     unlimited",
    "dmdba  soft  data     unlimited",
    "dmdba  hard  data     unlimited",
];

const SYSCTL_PARAMS: &[(&str, &str)] = &[
    ("fs.file-max",                  "fs.file-max = 6815744"),
    ("fs.aio-max-nr",                "fs.aio-max-nr = 1048576"),
    ("kernel.shmmni",                "kernel.shmmni = 4096"),
    ("kernel.sem",                   "kernel.sem = 250 32000 100 128"),
    ("net.ipv4.ip_local_port_range", "net.ipv4.ip_local_port_range = 9000 65500"),
    ("net.core.rmem_default",        "net.core.rmem_default = 4194304"),
    ("net.core.rmem_max",            "net.core.rmem_max = 4194304"),
    ("net.core.wmem_default",        "net.core.wmem_default = 262144"),
    ("net.core.wmem_max",            "net.core.wmem_max = 1048576"),
    ("vm.swappiness",                "vm.swappiness = 0"),
    ("vm.dirty_background_ratio",    "vm.dirty_background_ratio = 3"),
    ("vm.dirty_ratio",               "vm.dirty_ratio = 80"),
    ("vm.dirty_expire_centisecs",    "vm.dirty_expire_centisecs = 500"),
    ("vm.dirty_writeback_centisecs", "vm.dirty_writeback_centisecs = 100"),
];

// ── 本地工具函数 ──────────────────────────────────────────────────────────────

fn cmd_succeeds(program: &str, args: &[&str]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 以 root 或 sudo 执行命令。ctx 为空时忽略失败。
fn run_priv(use_sudo: bool, program: &str, args: &[&str], ctx: &str) -> Result<()> {
    let status = if use_sudo {
        std::process::Command::new("sudo").arg(program).args(args).status()
    } else {
        std::process::Command::new(program).args(args).status()
    };
    let status = status.with_context(|| {
        if ctx.is_empty() { program.to_string() } else { ctx.to_string() }
    })?;
    if !status.success() && !ctx.is_empty() {
        anyhow::bail!("{ctx}，退出码: {:?}", status.code());
    }
    Ok(())
}

/// 向程序的 stdin 写入数据（用于 chpasswd、tee 等管道场景）。
fn pipe_to_priv(use_sudo: bool, input: &[u8], program: &str, args: &[&str], ctx: &str) -> Result<()> {
    use std::io::Write;
    let mut cmd = if use_sudo {
        let mut c = std::process::Command::new("sudo");
        c.arg(program).args(args);
        c
    } else {
        let mut c = std::process::Command::new(program);
        c.args(args);
        c
    };
    let mut child = cmd
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::null())
    .spawn()
    .with_context(|| ctx.to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).with_context(|| ctx.to_string())?;
    }
    let status = child.wait().with_context(|| ctx.to_string())?;
    if !status.success() {
        anyhow::bail!("{ctx}，退出码: {:?}", status.code());
    }
    Ok(())
}

/// 向文件末尾追加内容（sudo 时用 tee -a）。
fn append_priv(use_sudo: bool, path: &str, content: &str, ctx: &str) -> Result<()> {
    if use_sudo {
        pipe_to_priv(use_sudo, content.as_bytes(), "tee", &["-a", path], ctx)
    } else {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .with_context(|| format!("打开 {path} 失败"))?;
        file.write_all(content.as_bytes()).with_context(|| ctx.to_string())
    }
}

fn compute_shm() -> Result<(u64, u64)> {
    let content = std::fs::read_to_string("/proc/meminfo").context("读取 /proc/meminfo 失败")?;
    let mem_kb: u64 = content
        .lines()
        .find(|l| l.starts_with("MemTotal:"))
        .context("未找到 MemTotal")?
        .split_whitespace()
        .nth(1)
        .context("MemTotal 格式异常")?
        .parse()
        .context("MemTotal 解析失败")?;
    Ok(((mem_kb as f64 * 0.64 / 4.0).round() as u64,
        (mem_kb as f64 * 0.64 * 1024.0).round() as u64))
}

// ── 远端工具函数 ──────────────────────────────────────────────────────────────

async fn exec_remote(runner: &dyn CommandRunner, cmd: &str, ctx: &str) -> Result<()> {
    runner.exec(cmd).await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("{ctx}: {e}"))
}

async fn remote_read_str(runner: &dyn CommandRunner, cmd: &str) -> String {
    runner.exec(cmd).await
        .map(|(bytes, _)| String::from_utf8_lossy(&bytes).trim().to_string())
        .unwrap_or_default()
}
