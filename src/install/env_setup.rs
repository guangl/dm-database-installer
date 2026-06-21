use anyhow::Result;

use crate::ssh::CommandRunner;
use crate::ui;

pub async fn run(runner: &dyn CommandRunner) -> Result<()> {
    check_privilege(runner).await?;
    setup_dmdba_user(runner).await?;
    disable_selinux(runner).await?;
    disable_thp(runner).await?;
    set_timezone(runner).await?;
    set_locale(runner).await?;
    optimize_sshd(runner).await?;
    disable_firewall(runner).await?;
    configure_limits(runner).await?;
    configure_pam(runner).await?;
    configure_sysctl(runner).await?;
    Ok(())
}

// ── 权限检测 ──────────────────────────────────────────────────────────────────

async fn check_privilege(runner: &dyn CommandRunner) -> Result<()> {
    let uid = read_str(runner, "id -u").await;
    if uid.trim() == "0" {
        ui::check_ok("root 权限", "");
        return Ok(());
    }
    let sudo_ok = read_str(runner, "sudo -n true 2>/dev/null && echo ok || echo fail").await;
    if sudo_ok.trim() == "ok" {
        ui::check_ok("root 权限", "通过 sudo");
        return Ok(());
    }
    anyhow::bail!(
        "环境准备需要 root 权限，当前用户既非 root 也无免密 sudo。\n\
         请以 root 身份重新运行，或执行以下命令为当前用户授权：\n\
         echo '$USER ALL=(ALL) NOPASSWD:ALL' | sudo tee /etc/sudoers.d/$USER"
    )
}

// ── dmdba 用户 ────────────────────────────────────────────────────────────────

// groupadd/useradd 依赖 /etc/{passwd,group,shadow,gshadow}.lock；若此前进程被强杀
// 或异常中断，会残留死锁文件，导致后续命令永久报错 "existing lock file ... with PID"。
// 这里检测锁文件中的 PID 是否仍存活，不存活则视为残留锁并清除（与 install.sh 一致）。
async fn clear_stale_account_locks(runner: &dyn CommandRunner) -> Result<()> {
    exec_r(
        runner,
        "for lock in /etc/passwd.lock /etc/group.lock /etc/shadow.lock /etc/gshadow.lock /etc/subuid.lock /etc/subgid.lock; do \
           [ -f \"$lock\" ] || continue; \
           pid=$(tr -dc '0-9' < \"$lock\" 2>/dev/null); \
           if [ -z \"$pid\" ] || ! kill -0 \"$pid\" 2>/dev/null; then rm -f \"$lock\"; fi; \
         done",
        "清除残留账户锁文件失败",
    )
    .await
}

async fn setup_dmdba_user(runner: &dyn CommandRunner) -> Result<()> {
    clear_stale_account_locks(runner).await?;
    exec_r(
        runner,
        "getent group dinstall >/dev/null 2>&1 || groupadd -g 1002 dinstall",
        "创建用户组 dinstall 失败",
    )
    .await?;
    exec_r(runner,
        "id dmdba >/dev/null 2>&1 || useradd -u 1002 -g dinstall -m -d /home/dmdba -s /bin/bash dmdba",
        "创建用户 dmdba 失败").await?;
    exec_r(
        runner,
        "echo 'dmdba:dmdba' | chpasswd",
        "设置 dmdba 密码失败",
    )
    .await?;
    exec_r(runner, "chage -M -1 dmdba", "设置密码永不过期失败").await?;
    let uid_out = read_str(runner, "id -u dmdba 2>/dev/null || echo fail").await;
    if uid_out.trim() == "fail" {
        anyhow::bail!("dmdba 用户创建后验证失败");
    }
    ui::log_ok("系统用户 dmdba 已就绪");
    Ok(())
}

// ── SELinux ───────────────────────────────────────────────────────────────────

async fn disable_selinux(runner: &dyn CommandRunner) -> Result<()> {
    exec_r(runner,
        "setenforce 0 2>/dev/null || true; \
         [ -f /etc/selinux/config ] && sed -i '/^SELINUX=/cSELINUX=disabled' /etc/selinux/config || true",
        "禁用 SELinux 失败").await?;
    let mode = read_str(runner, "getenforce 2>/dev/null || echo Disabled").await;
    if mode.trim() == "Enforcing" {
        anyhow::bail!("SELinux 运行时仍为 Enforcing，setenforce 0 未生效");
    }
    let label = if mode.trim().is_empty() || mode.trim() == "Disabled" {
        "已禁用"
    } else {
        mode.trim()
    };
    ui::check_ok("SELinux", label);
    Ok(())
}

// ── Transparent Hugepages ─────────────────────────────────────────────────────

async fn disable_thp(runner: &dyn CommandRunner) -> Result<()> {
    exec_r(
        runner,
        "[ -f /sys/kernel/mm/transparent_hugepage/enabled ] \
         && echo never | tee /sys/kernel/mm/transparent_hugepage/enabled >/dev/null || true; \
         if [ -f /etc/rc.local ]; then \
           grep -q 'transparent_hugepage' /etc/rc.local 2>/dev/null \
           || echo 'echo never > /sys/kernel/mm/transparent_hugepage/enabled' >> /etc/rc.local; \
           chmod +x /etc/rc.local; \
         fi",
        "关闭 Transparent Hugepages 失败",
    )
    .await?;
    let val = read_str(
        runner,
        "cat /sys/kernel/mm/transparent_hugepage/enabled 2>/dev/null || echo ok",
    )
    .await;
    if !val.is_empty() && !val.contains("never") && val.trim() != "ok" {
        anyhow::bail!("THP 关闭失败，当前值: {}", val.trim());
    }
    ui::check_ok("THP", "已关闭");
    Ok(())
}

// ── 时区 ──────────────────────────────────────────────────────────────────────

async fn set_timezone(runner: &dyn CommandRunner) -> Result<()> {
    exec_r(
        runner,
        "timedatectl set-timezone Asia/Shanghai 2>/dev/null || true",
        "设置时区失败",
    )
    .await?;
    let val = read_str(
        runner,
        "timedatectl show --property=Timezone 2>/dev/null || echo ''",
    )
    .await;
    if !val.is_empty() && !val.contains("Asia/Shanghai") {
        anyhow::bail!("时区设置验证失败，当前值: {}", val.trim());
    }
    ui::check_ok("时区", "Asia/Shanghai");
    Ok(())
}

// ── 字符集 ────────────────────────────────────────────────────────────────────

async fn set_locale(runner: &dyn CommandRunner) -> Result<()> {
    exec_r(
        runner,
        "grep -q 'export LANG=zh_CN.UTF-8' /etc/profile 2>/dev/null \
         || echo 'export LANG=zh_CN.UTF-8' >> /etc/profile",
        "设置字符集失败",
    )
    .await?;
    let val = read_str(
        runner,
        "grep 'LANG=zh_CN.UTF-8' /etc/profile 2>/dev/null || echo ''",
    )
    .await;
    if val.trim().is_empty() {
        anyhow::bail!("/etc/profile 中 LANG 设置写入验证失败");
    }
    ui::check_ok("字符集", "zh_CN.UTF-8");
    Ok(())
}

// ── SSH 优化 ──────────────────────────────────────────────────────────────────

async fn optimize_sshd(runner: &dyn CommandRunner) -> Result<()> {
    exec_r(
        runner,
        "if [ -f /etc/ssh/sshd_config ]; then \
           sed -i '/^#GSSAPIAuthentication/cGSSAPIAuthentication no' /etc/ssh/sshd_config; \
           sed -i '/^GSSAPIAuthentication/cGSSAPIAuthentication no'  /etc/ssh/sshd_config; \
           sed -i '/^#UseDNS/cUseDNS no'                            /etc/ssh/sshd_config; \
           sed -i '/^UseDNS/cUseDNS no'                             /etc/ssh/sshd_config; \
           systemctl reload sshd 2>/dev/null || true; \
         fi",
        "优化 sshd 配置失败",
    )
    .await?;
    ui::check_ok("SSH", "GSSAPIAuthentication=no  UseDNS=no");
    Ok(())
}

// ── 防火墙 ────────────────────────────────────────────────────────────────────

async fn disable_firewall(runner: &dyn CommandRunner) -> Result<()> {
    exec_r(
        runner,
        "systemctl stop firewalld 2>/dev/null || true; \
         systemctl disable firewalld 2>/dev/null || true",
        "关闭防火墙失败",
    )
    .await?;
    let state = read_str(
        runner,
        "systemctl is-active firewalld 2>/dev/null || echo inactive",
    )
    .await;
    if state.trim() == "active" {
        anyhow::bail!("防火墙关闭失败，当前状态仍为 active");
    }
    ui::check_ok("防火墙", "已关闭");
    Ok(())
}

// ── limits.conf ───────────────────────────────────────────────────────────────

async fn configure_limits(runner: &dyn CommandRunner) -> Result<()> {
    for &line in LIMITS_LINES {
        let cmd = format!(
            "grep -qF '{line}' /etc/security/limits.conf 2>/dev/null \
             || echo '{line}' >> /etc/security/limits.conf"
        );
        exec_r(runner, &cmd, "配置 limits.conf 失败").await?;
    }
    let last = LIMITS_LINES.last().unwrap();
    let check = read_str(
        runner,
        &format!("grep -cF '{last}' /etc/security/limits.conf 2>/dev/null || echo 0"),
    )
    .await;
    if check.trim() == "0" {
        anyhow::bail!("limits.conf 写入验证失败");
    }
    ui::check_ok("limits.conf", "nofile/nproc=65536");
    Ok(())
}

// ── PAM ───────────────────────────────────────────────────────────────────────

async fn configure_pam(runner: &dyn CommandRunner) -> Result<()> {
    exec_r(runner,
        r#"if [ -f /etc/pam.d/login ] && ! grep -q 'pam_limits.so' /etc/pam.d/login 2>/dev/null; then
  printf '\nsession    required        /lib64/security/pam_limits.so\nsession    required        pam_limits.so\n' >> /etc/pam.d/login
fi"#,
        "配置 PAM limits 失败").await?;
    let val = read_str(
        runner,
        "grep -c 'pam_limits.so' /etc/pam.d/login 2>/dev/null || echo 0",
    )
    .await;
    if val.trim() == "0" {
        anyhow::bail!("/etc/pam.d/login 中 pam_limits.so 写入验证失败");
    }
    ui::check_ok("PAM limits", "已配置");
    Ok(())
}

// ── sysctl.conf ───────────────────────────────────────────────────────────────

async fn configure_sysctl(runner: &dyn CommandRunner) -> Result<()> {
    for &(key, line) in SYSCTL_PARAMS {
        let cmd = format!(
            "grep -q '^{key}' /etc/sysctl.conf 2>/dev/null || echo '{line}' >> /etc/sysctl.conf"
        );
        exec_r(runner, &cmd, "配置 sysctl.conf 失败").await?;
    }
    let has_shmall = read_str(
        runner,
        "grep -c '^kernel.shmall' /etc/sysctl.conf 2>/dev/null || echo 0",
    )
    .await;
    if has_shmall.trim() == "0" || has_shmall.trim().is_empty() {
        let (shmall, shmmax) = compute_shm(runner).await;
        exec_r(
            runner,
            &format!("echo 'kernel.shmall={shmall}' >> /etc/sysctl.conf"),
            "配置 kernel.shmall 失败",
        )
        .await?;
        exec_r(
            runner,
            &format!("echo 'kernel.shmmax={shmmax}' >> /etc/sysctl.conf"),
            "配置 kernel.shmmax 失败",
        )
        .await?;
    }
    exec_r(
        runner,
        "sysctl -p >/dev/null 2>&1 || true",
        "应用 sysctl 参数失败",
    )
    .await?;
    let val = read_str(runner, "sysctl fs.file-max 2>/dev/null || echo ''").await;
    if !val.is_empty() && !val.contains("6815744") {
        anyhow::bail!("sysctl fs.file-max 未生效，当前: {}", val.trim());
    }
    ui::check_ok("sysctl", "内核参数已生效");
    Ok(())
}

async fn compute_shm(runner: &dyn CommandRunner) -> (u64, u64) {
    let meminfo = read_str(
        runner,
        "grep '^MemTotal:' /proc/meminfo 2>/dev/null || echo ''",
    )
    .await;
    let mem_kb: u64 = meminfo
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4 * 1024 * 1024);
    (
        (mem_kb as f64 * 0.64 / 4.0).round() as u64,
        (mem_kb as f64 * 0.64 * 1024.0).round() as u64,
    )
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
    ("fs.file-max", "fs.file-max = 6815744"),
    ("fs.aio-max-nr", "fs.aio-max-nr = 1048576"),
    ("kernel.shmmni", "kernel.shmmni = 4096"),
    ("kernel.sem", "kernel.sem = 250 32000 100 128"),
    (
        "net.ipv4.ip_local_port_range",
        "net.ipv4.ip_local_port_range = 9000 65500",
    ),
    ("net.core.rmem_default", "net.core.rmem_default = 4194304"),
    ("net.core.rmem_max", "net.core.rmem_max = 4194304"),
    ("net.core.wmem_default", "net.core.wmem_default = 262144"),
    ("net.core.wmem_max", "net.core.wmem_max = 1048576"),
    ("vm.swappiness", "vm.swappiness = 0"),
    ("vm.dirty_background_ratio", "vm.dirty_background_ratio = 3"),
    ("vm.dirty_ratio", "vm.dirty_ratio = 80"),
    (
        "vm.dirty_expire_centisecs",
        "vm.dirty_expire_centisecs = 500",
    ),
    (
        "vm.dirty_writeback_centisecs",
        "vm.dirty_writeback_centisecs = 100",
    ),
];

// ── 工具函数 ──────────────────────────────────────────────────────────────────

async fn exec_r(runner: &dyn CommandRunner, cmd: &str, ctx: &str) -> Result<()> {
    runner
        .exec(cmd)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("{ctx}: {e}"))
}

async fn read_str(runner: &dyn CommandRunner, cmd: &str) -> String {
    runner
        .exec(cmd)
        .await
        .map(|(bytes, _)| String::from_utf8_lossy(&bytes).trim().to_string())
        .unwrap_or_default()
}
