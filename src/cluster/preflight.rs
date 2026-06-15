use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use futures::future::join_all;

use crate::common::ssh::{CommandRunner, SshError};
use crate::config::cluster::{DminitConfig, NodeConfig};

/// 检查节点是否具备 sudo 免密权限。
pub async fn check_sudo_nopass(runner: &dyn CommandRunner) -> Result<()> {
    tracing::debug!("[预检查] 检测 sudo 免密权限");
    match runner.exec("sudo -n true").await {
        Ok(_) => {
            tracing::debug!("[预检查] sudo 免密权限通过");
            Ok(())
        }
        Err(_) => bail!("[预检查] sudo 免密失败：目标节点需要无密码 sudo 权限"),
    }
}

/// 检查指定端口是否未被占用。
pub async fn check_port_available(runner: &dyn CommandRunner, port: u16) -> Result<()> {
    tracing::debug!("[预检查] 检测端口 {} 是否空闲", port);
    let cmd = format!("ss -tlnp | grep ':{port}'");
    match runner.exec(&cmd).await {
        Ok((stdout, _)) if !stdout.is_empty() => {
            bail!("[预检查] 端口 {} 已被占用", port)
        }
        Ok(_) => {
            tracing::debug!("[预检查] 端口 {} 空闲", port);
            Ok(())
        }
        // grep 返回 exit_code 1 表示无匹配（端口空闲），也是 Ok
        Err(crate::common::ssh::SshError::ExecFailed { exit_code: 1, .. }) => {
            tracing::debug!("[预检查] 端口 {} 空闲", port);
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

/// 检查安装路径父目录的磁盘剩余空间（要求 >= 20 GB）。
pub async fn check_disk_space(runner: &dyn CommandRunner, install_path: &str) -> Result<()> {
    let parent = Path::new(install_path)
        .parent()
        .unwrap_or_else(|| Path::new("/"));
    tracing::debug!("[预检查] 检测磁盘空间: {}", parent.display());
    let cmd = format!("df -B1 {}", parent.display());
    let (stdout, _) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let available = parse_df_available(&stdout)?;
    let min_bytes: u64 = 20 * 1024 * 1024 * 1024;
    tracing::debug!(
        "[预检查] 磁盘剩余: {} GB，最低要求: 20 GB",
        available / (1024 * 1024 * 1024)
    );
    if available < min_bytes {
        bail!(
            "[预检查] 磁盘空间不足: 剩余 {} bytes，需要 >= 20 GB",
            available
        );
    }
    Ok(())
}

/// 检查节点总内存（要求 MemTotal >= 4 GB）。
pub async fn check_memory(runner: &dyn CommandRunner) -> Result<()> {
    tracing::debug!("[预检查] 检测内存大小");
    let (stdout, _) = runner
        .exec("grep '^MemTotal:' /proc/meminfo")
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let total_kb = parse_memtotal_kb(&stdout)?;
    let min_kb: u64 = 4 * 1024 * 1024;
    tracing::debug!(
        "[预检查] 内存总量: {} GB，最低要求: 4 GB",
        total_kb / (1024 * 1024)
    );
    if total_kb < min_kb {
        bail!("[预检查] 内存不足: {} KB，需要 >= 4 GB", total_kb);
    }
    Ok(())
}

fn parse_memtotal_kb(stdout: &[u8]) -> Result<u64> {
    let text = std::str::from_utf8(stdout).context("/proc/meminfo 输出不是有效 UTF-8")?;
    let line = text.lines().next().context("/proc/meminfo 输出为空")?;
    line.split_whitespace()
        .nth(1)
        .context("MemTotal 行格式异常")?
        .parse::<u64>()
        .context("MemTotal 值无法解析为 u64")
}

/// 检查节点 CPU 核心数（要求 >= 1 核）。
pub async fn check_cpu_cores(runner: &dyn CommandRunner) -> Result<()> {
    tracing::debug!("[预检查] 检测 CPU 核心数");
    let (stdout, _) = runner
        .exec("nproc")
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let text = std::str::from_utf8(&stdout).context("nproc 输出不是有效 UTF-8")?;
    let cores: u64 = text.trim().parse().context("nproc 输出无法解析为整数")?;
    tracing::debug!("[预检查] CPU 核心数: {}", cores);
    if cores < 1 {
        bail!("[预检查] CPU 核心数不足: {} 核，需要 >= 1 核", cores);
    }
    Ok(())
}

/// 解析 `df -B1` 输出的第二行第 4 列（Available bytes）。
fn parse_df_available(stdout: &[u8]) -> Result<u64> {
    let text = std::str::from_utf8(stdout).context("df 输出不是有效 UTF-8")?;
    let second_line = text
        .lines()
        .nth(1)
        .context("df 输出行数不足，无法解析 Available 列")?;
    let available_str = second_line
        .split_whitespace()
        .nth(3)
        .context("df 输出列数不足，无法解析第 4 列")?;
    available_str
        .parse::<u64>()
        .context(format!("df Available 列无法解析为 u64: {available_str}"))
}

/// 检查 nofile / nproc soft limit（< 65536 时记录 warn，不阻断部署）。
pub async fn check_ulimits(runner: &dyn CommandRunner) -> Result<()> {
    tracing::debug!("[预检查] 检测 ulimit (nofile/nproc)");
    let cmd = "awk '/^Max open files/{print $4} /^Max processes/{print $3}' /proc/self/limits";
    let (out, _) = runner.exec(cmd).await.map_err(|e| anyhow::anyhow!(e))?;
    let text = std::str::from_utf8(&out).unwrap_or("").trim().to_string();
    let mut lines = text.lines();
    warn_ulimit_if_low("nofile", lines.next().unwrap_or("unlimited"));
    warn_ulimit_if_low("nproc",  lines.next().unwrap_or("unlimited"));
    Ok(())
}

fn warn_ulimit_if_low(name: &str, val_str: &str) {
    const MIN: u64 = 65536;
    let val: u64 = match val_str.trim().parse() {
        Ok(n) => n,
        Err(_) => return, // "unlimited" 或解析失败 → 视为无限制
    };
    if val < MIN {
        tracing::warn!(
            "[预检查] {} soft limit = {}，建议 >= {}；\
             请在 /etc/security/limits.conf 中添加: dmdba soft {} {}",
            name, val, MIN, name, MIN
        );
    }
}

/// 检查 SELinux 模式（Enforcing 时记录 warn，不阻断部署）。
pub async fn check_selinux(runner: &dyn CommandRunner) -> Result<()> {
    tracing::debug!("[预检查] 检测 SELinux 状态");
    let (out, _) = match runner.exec("getenforce 2>/dev/null || echo absent").await {
        Ok(r) => r,
        Err(_) => {
            tracing::debug!("[预检查] getenforce 不可用，跳过 SELinux 检测");
            return Ok(());
        }
    };
    let mode = std::str::from_utf8(&out).unwrap_or("").trim();
    if mode == "Enforcing" {
        tracing::warn!(
            "[预检查] SELinux 处于 Enforcing 模式，可能阻断 DM 进程启动；\
             临时切换: setenforce 0；\
             永久禁用: 将 /etc/selinux/config 中 SELINUX=enforcing 改为 permissive 并重启"
        );
    } else {
        tracing::debug!("[预检查] SELinux 状态: {}（不影响安装）", if mode.is_empty() { "未知" } else { mode });
    }
    Ok(())
}

/// 检查 NTP 时钟同步状态（仅 systemd 系统支持；无 timedatectl 时跳过并打 warn）。
pub async fn check_time_sync(runner: &dyn CommandRunner) -> Result<()> {
    tracing::debug!("[预检查] 检测 NTP 时钟同步状态");
    let cmd = "timedatectl show --property=NTPSynchronized --value 2>/dev/null";
    let out = match runner.exec(cmd).await {
        Ok((out, _)) => out,
        Err(_) => {
            tracing::warn!("[预检查] timedatectl 不可用，跳过时钟同步检测（建议手动确认所有节点已启用 NTP）");
            return Ok(());
        }
    };
    let status = std::str::from_utf8(&out).unwrap_or("").trim();
    if status == "no" {
        bail!("[预检查] NTP 时钟未同步：集群节点须配置 NTP/chrony，时钟偏差可能导致主备切换异常");
    }
    tracing::debug!(
        "[预检查] NTP 时钟同步状态: {}",
        if status.is_empty() { "未知（跳过）" } else { status }
    );
    Ok(())
}

/// 检查是否有 DM 数据库进程正在运行（dmserver / dmwatcher / dmasmsvr）。
pub async fn check_existing_dm_process(runner: &dyn CommandRunner) -> Result<()> {
    tracing::debug!("[预检查] 检测 DM 进程是否在运行");
    let cmd = "ps -eo comm= | grep -E '^(dmserver|dmwatcher|dmasmsvr)$' || true";
    let (out, _) = runner.exec(cmd).await.map_err(|e| anyhow::anyhow!(e))?;
    let running = std::str::from_utf8(&out).unwrap_or("").trim();
    if !running.is_empty() {
        let names = running.lines().collect::<Vec<_>>().join(", ");
        bail!(
            "[预检查] 检测到 DM 进程正在运行: [{}]；请先停止现有服务再重新部署",
            names
        );
    }
    tracing::debug!("[预检查] 未检测到 DM 进程");
    Ok(())
}

/// 检查 dmdba 操作系统用户和组是否已存在（存在则 warn，不阻断部署）。
pub async fn check_dmdba_user(runner: &dyn CommandRunner) -> Result<()> {
    tracing::debug!("[预检查] 检测 dmdba 用户/组是否已存在");
    match runner.exec("id dmdba").await {
        Ok((out, _)) => {
            let info = std::str::from_utf8(&out).unwrap_or("").trim().to_string();
            tracing::warn!(
                "[预检查] dmdba 用户已存在: {}；可能存在旧版 DM 安装，请确认不会与本次部署冲突",
                info
            );
        }
        Err(SshError::ExecFailed { exit_code: 1, .. }) => {
            tracing::debug!("[预检查] dmdba 用户不存在，安装程序将自动创建");
        }
        Err(e) => return Err(anyhow::anyhow!(e)),
    }
    match runner.exec("getent group dmdba").await {
        Ok(_) => {
            tracing::warn!("[预检查] dmdba 组已存在；若与现有配置不一致可能导致权限问题");
        }
        Err(SshError::ExecFailed { exit_code: 2, .. }) => {
            tracing::debug!("[预检查] dmdba 组不存在，安装程序将自动创建");
        }
        Err(e) => return Err(anyhow::anyhow!(e)),
    }
    Ok(())
}

/// 检查内核参数 vm.swappiness 和 kernel.shmmax，偏低时 warn 但不阻断部署。
pub async fn check_kernel_params(runner: &dyn CommandRunner) -> Result<()> {
    tracing::debug!("[预检查] 检测内核参数（vm.swappiness / kernel.shmmax）");
    if let Ok((out, _)) = runner.exec("sysctl -n vm.swappiness").await {
        let val: u64 = std::str::from_utf8(&out).unwrap_or("").trim().parse().unwrap_or(0);
        if val > 10 {
            tracing::warn!(
                "[预检查] vm.swappiness = {}，建议 <= 10；\
                 永久生效: 在 /etc/sysctl.conf 添加 vm.swappiness=10，然后执行 sysctl -p",
                val
            );
        }
    }
    if let Ok((out, _)) = runner.exec("sysctl -n kernel.shmmax").await {
        const MIN_SHMMAX: u64 = 2 * 1024 * 1024 * 1024;
        let val: u64 = std::str::from_utf8(&out).unwrap_or("").trim().parse().unwrap_or(u64::MAX);
        if val < MIN_SHMMAX {
            tracing::warn!(
                "[预检查] kernel.shmmax = {} bytes（约 {} GB），建议 >= 2 GB；\
                 永久生效: 在 /etc/sysctl.conf 添加 kernel.shmmax={}，然后执行 sysctl -p",
                val,
                val / (1024 * 1024 * 1024),
                MIN_SHMMAX
            );
        }
    }
    Ok(())
}

/// 对单个节点执行全部预检查，任一硬性检查失败即返回带节点信息的 Err。
pub async fn check_node(node: &NodeConfig, dminit: &DminitConfig, runner: &dyn CommandRunner) -> Result<()> {
    let ctx = || format!("节点 {} ({:?}) 预检查失败", node.host, node.role);
    check_existing_dm_process(runner).await.with_context(ctx)?;
    check_sudo_nopass(runner).await.with_context(ctx)?;
    check_memory(runner).await.with_context(ctx)?;
    check_cpu_cores(runner).await.with_context(ctx)?;
    check_port_available(runner, dminit.port).await.with_context(ctx)?;
    check_port_available(runner, node.mal_port).await.with_context(ctx)?;
    check_port_available(runner, node.dw_port).await.with_context(ctx)?;
    check_port_available(runner, node.inst_dw_port).await.with_context(ctx)?;
    check_time_sync(runner).await.with_context(ctx)?;
    check_disk_space(runner, &dminit.install_path).await.with_context(ctx)?;
    check_ulimits(runner).await.with_context(ctx)?;
    check_selinux(runner).await.with_context(ctx)?;
    check_dmdba_user(runner).await.with_context(ctx)?;
    check_kernel_params(runner).await.with_context(ctx)?;
    Ok(())
}

/// 检查当前节点是否能 TCP 连通所有其他节点的 mal_port（探测防火墙/路由配置）。
pub async fn check_inter_node_connectivity(
    items: &[(NodeConfig, Arc<dyn CommandRunner>)],
) -> Result<()> {
    if items.len() <= 1 {
        return Ok(());
    }
    tracing::info!("[预检查] 检测节点间 MAL 网络互通性（{} 个节点）", items.len());
    let mut failures: Vec<String> = Vec::new();
    for (src_node, runner) in items {
        for (dst_node, _) in items {
            if src_node.host == dst_node.host {
                continue;
            }
            let cmd = format!(
                "bash -c 'echo > /dev/tcp/{}/{} 2>/dev/null'",
                dst_node.host, dst_node.mal_port,
            );
            match runner.exec(&cmd).await {
                Ok(_) => {
                    tracing::debug!(
                        "[预检查] {} -> {}:{} (mal_port) 连通",
                        src_node.host, dst_node.host, dst_node.mal_port
                    );
                }
                Err(SshError::ExecFailed { .. }) => {
                    failures.push(format!(
                        "  {} 无法连通 {}:{} (mal_port)",
                        src_node.host, dst_node.host, dst_node.mal_port
                    ));
                }
                Err(e) => {
                    tracing::warn!("[预检查] {} 连通性检测命令执行异常（跳过）: {e}", src_node.host);
                }
            }
        }
    }
    if !failures.is_empty() {
        bail!("[预检查] 节点间 MAL 网络不通，请检查防火墙规则:\n{}", failures.join("\n"));
    }
    tracing::info!("[预检查] 节点间网络互通检查通过");
    Ok(())
}

/// 并发对所有节点执行预检查，收集所有失败节点后统一报告，最后检查节点间网络互通。
pub async fn preflight_all_nodes(
    items: Vec<(NodeConfig, Arc<dyn CommandRunner>)>,
    dminit: &DminitConfig,
) -> Result<()> {
    tracing::info!("开始并发预检查，共 {} 个节点", items.len());
    let futures = items.iter().map(|(node, runner)| {
        let node = node.clone();
        let runner = Arc::clone(runner);
        let dminit = dminit.clone();
        async move { check_node(&node, &dminit, runner.as_ref()).await }
    });
    let results: Vec<Result<()>> = join_all(futures).await;
    let failures: Vec<String> = results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            r.as_ref()
                .err()
                .map(|e| format!("[{}] {:#}", items[i].0.host, e))
        })
        .collect();
    if !failures.is_empty() {
        bail!("预检查失败 — 中止部署:\n{}", failures.join("\n"));
    }
    check_inter_node_connectivity(&items).await?;
    tracing::info!("所有节点预检查通过");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ssh::MockRunner;
    use crate::config::cluster::{DminitConfig, NodeConfig, NodeRole, SshCredentials};

    fn make_dminit() -> DminitConfig {
        DminitConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            port: 5236,
            page_size: 8,
            charset: 0,
            case_sensitive: true,
            extent_size: 16,
            sysdba_password: "SYSDBA".to_string(),
        }
    }

    fn make_node() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Primary,
            host: "192.168.1.10".to_string(),
            instance_name: "DMSVR01".to_string(),
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            read_only: false,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: None,
                password: Some("pass".to_string()),
                port: 22,
            },
        }
    }

    fn df_output_with_available(available_bytes: u64) -> Vec<u8> {
        format!(
            "Filesystem  1B-blocks  Used  Available  Use%  Mounted on\n/dev/sda1  100000000000  50000000000  {}  50%  /opt\n",
            available_bytes
        )
        .into_bytes()
    }

    fn mem_output_kb(kb: u64) -> Vec<u8> {
        format!("MemTotal:       {} kB\n", kb).into_bytes()
    }

    #[tokio::test]
    async fn test_check_node_all_pass() {
        let df_out = df_output_with_available(25 * 1024 * 1024 * 1024);
        let runner = MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("grep '^MemTotal:'".to_string(), 0, mem_output_kb(8 * 1024 * 1024)),
            ("nproc".to_string(), 0, b"4\n".to_vec()),
            ("ss -tlnp | grep ':5236'".to_string(), 1, vec![]),
            ("df -B1 /opt".to_string(), 0, df_out),
        ]);
        let node = make_node();
        let result = check_node(&node, &make_dminit(), &runner).await;
        assert!(result.is_ok(), "五项全通过应返回 Ok: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_check_node_sudo_fail() {
        let runner = MockRunner::new(vec![
            ("sudo -n true".to_string(), 1, vec![]),
        ]);
        let node = make_node();
        let result = check_node(&node, &make_dminit(), &runner).await;
        assert!(result.is_err(), "sudo 失败应返回 Err");
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("192.168.1.10"), "应含节点 host: {msg}");
        assert!(msg.contains("sudo 免密"), "应含 'sudo 免密' 关键字: {msg}");
    }

    #[tokio::test]
    async fn test_check_node_port_occupied() {
        let runner = MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("grep '^MemTotal:'".to_string(), 0, mem_output_kb(8 * 1024 * 1024)),
            ("nproc".to_string(), 0, b"4\n".to_vec()),
            (
                "ss -tlnp | grep ':5236'".to_string(),
                0,
                b"LISTEN 0 128 *:5236 *:* users:((\"dmserver\",pid=1234))".to_vec(),
            ),
        ]);
        let node = make_node();
        let result = check_node(&node, &make_dminit(), &runner).await;
        assert!(result.is_err(), "端口被占应返回 Err");
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("端口 5236 已被占用"), "应含端口错误: {msg}");
    }

    #[tokio::test]
    async fn test_check_node_disk_insufficient() {
        let df_out = df_output_with_available(1_000_000);
        let runner = MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("grep '^MemTotal:'".to_string(), 0, mem_output_kb(8 * 1024 * 1024)),
            ("nproc".to_string(), 0, b"4\n".to_vec()),
            ("ss -tlnp | grep ':5236'".to_string(), 1, vec![]),
            ("df -B1 /opt".to_string(), 0, df_out),
        ]);
        let node = make_node();
        let result = check_node(&node, &make_dminit(), &runner).await;
        assert!(result.is_err(), "磁盘不足应返回 Err");
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("磁盘空间不足"), "应含磁盘错误: {msg}");
        assert!(msg.contains("20 GB"), "应含 20 GB 字样: {msg}");
    }

    #[tokio::test]
    async fn test_check_memory_insufficient() {
        let runner = MockRunner::new(vec![
            ("grep '^MemTotal:'".to_string(), 0, mem_output_kb(2 * 1024 * 1024)),
        ]);
        let result = check_memory(&runner).await;
        assert!(result.is_err(), "内存不足应返回 Err");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("内存不足"), "应含 '内存不足': {msg}");
        assert!(msg.contains("4 GB"), "应含 '4 GB': {msg}");
    }

    #[tokio::test]
    async fn test_check_cpu_cores_insufficient() {
        let runner = MockRunner::new(vec![
            ("nproc".to_string(), 0, b"0\n".to_vec()),
        ]);
        let result = check_cpu_cores(&runner).await;
        assert!(result.is_err(), "0 核应返回 Err");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("CPU 核心数不足"), "应含 'CPU 核心数不足': {msg}");
    }

    #[test]
    fn test_parse_memtotal_kb_normal() {
        let input = b"MemTotal:       8388608 kB\n";
        let result = parse_memtotal_kb(input).unwrap();
        assert_eq!(result, 8_388_608);
    }

    #[tokio::test]
    async fn test_check_time_sync_synchronized() {
        let runner = MockRunner::new(vec![
            ("timedatectl show --property=NTPSynchronized --value".to_string(), 0, b"yes\n".to_vec()),
        ]);
        let result = check_time_sync(&runner).await;
        assert!(result.is_ok(), "NTP 已同步应返回 Ok: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_check_time_sync_not_synchronized() {
        let runner = MockRunner::new(vec![
            ("timedatectl show --property=NTPSynchronized --value".to_string(), 0, b"no\n".to_vec()),
        ]);
        let result = check_time_sync(&runner).await;
        assert!(result.is_err(), "NTP 未同步应返回 Err");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("NTP 时钟未同步"), "应含时钟错误，实际: {msg}");
    }

    #[tokio::test]
    async fn test_check_time_sync_timedatectl_unavailable() {
        let runner = MockRunner::new_strict(vec![]);
        let result = check_time_sync(&runner).await;
        assert!(result.is_ok(), "timedatectl 不可用应跳过（返回 Ok）: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_check_inter_node_connectivity_single_node_skipped() {
        let runner = Arc::new(MockRunner::new_strict(vec![])) as Arc<dyn CommandRunner>;
        let node = make_node();
        let items = vec![(node, runner)];
        let result = check_inter_node_connectivity(&items).await;
        assert!(result.is_ok(), "单节点应跳过互通检查: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_check_inter_node_connectivity_pass() {
        let mut node_a = make_node();
        node_a.host = "192.168.1.10".to_string();
        let mut node_b = make_node();
        node_b.host = "192.168.1.11".to_string();
        node_b.mal_port = 5237;

        let runner_a = Arc::new(MockRunner::new(vec![
            (
                format!("bash -c 'echo > /dev/tcp/{}/{}", node_b.host, node_b.mal_port),
                0,
                vec![],
            ),
        ])) as Arc<dyn CommandRunner>;
        let runner_b = Arc::new(MockRunner::new(vec![
            (
                format!("bash -c 'echo > /dev/tcp/{}/{}", node_a.host, node_a.mal_port),
                0,
                vec![],
            ),
        ])) as Arc<dyn CommandRunner>;
        let items = vec![(node_a, runner_a), (node_b, runner_b)];
        let result = check_inter_node_connectivity(&items).await;
        assert!(result.is_ok(), "双节点互通应返回 Ok: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_check_inter_node_connectivity_fail() {
        let mut node_a = make_node();
        node_a.host = "192.168.1.10".to_string();
        let mut node_b = make_node();
        node_b.host = "192.168.1.11".to_string();
        node_b.mal_port = 5237;

        // node_a 无法连通 node_b
        let runner_a = Arc::new(MockRunner::new(vec![
            (
                format!("bash -c 'echo > /dev/tcp/{}/{}", node_b.host, node_b.mal_port),
                1,
                vec![],
            ),
        ])) as Arc<dyn CommandRunner>;
        let runner_b = Arc::new(MockRunner::new(vec![
            (
                format!("bash -c 'echo > /dev/tcp/{}/{}", node_a.host, node_a.mal_port),
                0,
                vec![],
            ),
        ])) as Arc<dyn CommandRunner>;
        let items = vec![(node_a, runner_a), (node_b, runner_b)];
        let result = check_inter_node_connectivity(&items).await;
        assert!(result.is_err(), "连通失败应返回 Err");
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("192.168.1.10"), "应含源节点 IP: {msg}");
        assert!(msg.contains("192.168.1.11"), "应含目标节点 IP: {msg}");
    }

    #[tokio::test]
    async fn test_check_existing_dm_process_not_running() {
        let runner = MockRunner::new(vec![
            ("ps -eo comm=".to_string(), 0, b"".to_vec()),
        ]);
        assert!(check_existing_dm_process(&runner).await.is_ok());
    }

    #[tokio::test]
    async fn test_check_existing_dm_process_dmserver_running() {
        let runner = MockRunner::new(vec![
            ("ps -eo comm=".to_string(), 0, b"dmserver\n".to_vec()),
        ]);
        let result = check_existing_dm_process(&runner).await;
        assert!(result.is_err(), "dmserver 在运行应返回 Err");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("dmserver"), "应含进程名: {msg}");
    }

    #[tokio::test]
    async fn test_check_existing_dm_process_multiple_procs() {
        let runner = MockRunner::new(vec![
            ("ps -eo comm=".to_string(), 0, b"dmserver\ndmwatcher\n".to_vec()),
        ]);
        let result = check_existing_dm_process(&runner).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("dmwatcher"), "应含 dmwatcher: {msg}");
    }

    #[tokio::test]
    async fn test_check_dmdba_user_not_exists() {
        let runner = MockRunner::new(vec![
            ("id dmdba".to_string(), 1, vec![]),
            ("getent group dmdba".to_string(), 2, vec![]),
        ]);
        assert!(check_dmdba_user(&runner).await.is_ok(), "用户不存在应返回 Ok（warn 免阻断）");
    }

    #[tokio::test]
    async fn test_check_dmdba_user_exists_warn_not_bail() {
        let runner = MockRunner::new(vec![
            ("id dmdba".to_string(), 0, b"uid=1001(dmdba) gid=1001(dmdba) groups=1001(dmdba)\n".to_vec()),
            ("getent group dmdba".to_string(), 0, b"dmdba:x:1001:\n".to_vec()),
        ]);
        assert!(check_dmdba_user(&runner).await.is_ok(), "用户存在仅 warn 不应 bail");
    }

    #[tokio::test]
    async fn test_check_kernel_params_swappiness_high_warn_ok() {
        let runner = MockRunner::new(vec![
            ("sysctl -n vm.swappiness".to_string(), 0, b"60\n".to_vec()),
            ("sysctl -n kernel.shmmax".to_string(), 0, b"4294967296\n".to_vec()),
        ]);
        assert!(check_kernel_params(&runner).await.is_ok(), "swappiness 偏高仅 warn 不阻断");
    }

    #[tokio::test]
    async fn test_check_kernel_params_shmmax_low_warn_ok() {
        let runner = MockRunner::new(vec![
            ("sysctl -n vm.swappiness".to_string(), 0, b"5\n".to_vec()),
            ("sysctl -n kernel.shmmax".to_string(), 0, b"1073741824\n".to_vec()),
        ]);
        assert!(check_kernel_params(&runner).await.is_ok(), "shmmax 偏低仅 warn 不阻断");
    }

    #[tokio::test]
    async fn test_check_kernel_params_all_ok() {
        let runner = MockRunner::new(vec![
            ("sysctl -n vm.swappiness".to_string(), 0, b"5\n".to_vec()),
            ("sysctl -n kernel.shmmax".to_string(), 0, b"4294967296\n".to_vec()),
        ]);
        assert!(check_kernel_params(&runner).await.is_ok());
    }

    #[tokio::test]
    async fn test_check_kernel_params_sysctl_unavailable() {
        let runner = MockRunner::new_strict(vec![]);
        assert!(check_kernel_params(&runner).await.is_ok(), "sysctl 不可用应跳过");
    }

    #[tokio::test]
    async fn test_check_ulimits_low_nofile_returns_ok() {
        // nofile 偏低时应 warn 但不 bail
        let runner = MockRunner::new(vec![
            ("awk".to_string(), 0, b"512\n65536\n".to_vec()),
        ]);
        let result = check_ulimits(&runner).await;
        assert!(result.is_ok(), "ulimit 偏低应返回 Ok（warn 但不阻断）: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_check_ulimits_all_sufficient_returns_ok() {
        let runner = MockRunner::new(vec![
            ("awk".to_string(), 0, b"65536\n65536\n".to_vec()),
        ]);
        assert!(check_ulimits(&runner).await.is_ok());
    }

    #[tokio::test]
    async fn test_check_ulimits_unlimited_returns_ok() {
        let runner = MockRunner::new(vec![
            ("awk".to_string(), 0, b"unlimited\nunlimited\n".to_vec()),
        ]);
        assert!(check_ulimits(&runner).await.is_ok());
    }

    #[tokio::test]
    async fn test_check_selinux_enforcing_returns_ok() {
        let runner = MockRunner::new(vec![
            ("getenforce".to_string(), 0, b"Enforcing\n".to_vec()),
        ]);
        let result = check_selinux(&runner).await;
        assert!(result.is_ok(), "SELinux Enforcing 应 warn 但返回 Ok: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_check_selinux_permissive_returns_ok() {
        let runner = MockRunner::new(vec![
            ("getenforce".to_string(), 0, b"Permissive\n".to_vec()),
        ]);
        assert!(check_selinux(&runner).await.is_ok());
    }

    #[tokio::test]
    async fn test_check_selinux_unavailable_returns_ok() {
        // strict 模式下 getenforce 不可用 → 应跳过并返回 Ok
        let runner = MockRunner::new_strict(vec![]);
        assert!(check_selinux(&runner).await.is_ok(), "getenforce 不可用应跳过");
    }

    #[tokio::test]
    async fn test_preflight_all_nodes_mixed() {
        let df_out = df_output_with_available(25 * 1024 * 1024 * 1024);
        let runner_ok = Arc::new(MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("grep '^MemTotal:'".to_string(), 0, mem_output_kb(8 * 1024 * 1024)),
            ("nproc".to_string(), 0, b"4\n".to_vec()),
            ("ss -tlnp | grep ':5236'".to_string(), 1, vec![]),
            ("df -B1 /opt".to_string(), 0, df_out),
        ])) as Arc<dyn CommandRunner>;

        let runner_fail = Arc::new(MockRunner::new(vec![
            ("sudo -n true".to_string(), 1, vec![]),
        ])) as Arc<dyn CommandRunner>;

        let mut node_ok = make_node();
        node_ok.host = "192.168.1.10".to_string();

        let mut node_fail = make_node();
        node_fail.host = "192.168.1.11".to_string();
        node_fail.role = NodeRole::Standby;

        let items = vec![(node_ok, runner_ok), (node_fail, runner_fail)];
        let result = preflight_all_nodes(items, &make_dminit()).await;
        assert!(result.is_err(), "有失败节点时应返回 Err");
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("192.168.1.11"), "应含失败节点 host: {msg}");
    }
}
