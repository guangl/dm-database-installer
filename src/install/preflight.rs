use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::ssh::{CommandRunner, SshError};
use crate::ui;

pub fn is_already_installed(install_path: &str) -> bool {
    Path::new(install_path).join("bin/dmserver").exists()
}

pub async fn check_port_available(runner: &dyn CommandRunner, port: u16) -> Result<()> {
    let cmd = format!("ss -tlnp | grep ':{port}'");
    match runner.exec(&cmd).await {
        Ok((stdout, _)) if !stdout.is_empty() => {
            bail!("[预检查] 端口 {} 已被占用", port)
        }
        Ok(_) => {
            ui::check_ok(&format!("端口 {}", port), "可用");
            Ok(())
        }
        Err(SshError::ExecFailed { exit_code: 1, .. }) => {
            ui::check_ok(&format!("端口 {}", port), "可用");
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

pub async fn check_disk_space(runner: &dyn CommandRunner, install_path: &str) -> Result<()> {
    let parent = Path::new(install_path)
        .parent()
        .unwrap_or_else(|| Path::new("/"));
    let cmd = format!("df -B1 {}", parent.display());
    let (stdout, _) = runner.exec(&cmd).await.map_err(|e| anyhow::anyhow!(e))?;
    let available = parse_df_available(&stdout)?;
    let min_bytes: u64 = 20 * 1024 * 1024 * 1024;
    if available < min_bytes {
        bail!(
            "[预检查] 磁盘空间不足: 剩余 {} bytes，需要 >= 20 GB",
            available
        );
    }
    let gb = available / (1024 * 1024 * 1024);
    ui::check_ok("安装路径磁盘", &format!("{} GB 可用", gb));
    Ok(())
}

pub async fn check_memory(runner: &dyn CommandRunner) -> Result<()> {
    let (stdout, _) = runner
        .exec("grep '^MemTotal:' /proc/meminfo")
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let total_kb = parse_memtotal_kb(&stdout)?;
    let min_kb: u64 = 4 * 1024 * 1024;
    if total_kb < min_kb {
        bail!("[预检查] 内存不足: {} KB，需要 >= 4 GB", total_kb);
    }
    let mb = total_kb / 1024;
    ui::check_ok("内存", &format!("{} MB", mb));
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

pub async fn check_cpu_cores(runner: &dyn CommandRunner) -> Result<()> {
    let (stdout, _) = runner.exec("nproc").await.map_err(|e| anyhow::anyhow!(e))?;
    let text = std::str::from_utf8(&stdout).context("nproc 输出不是有效 UTF-8")?;
    let cores: u64 = text.trim().parse().context("nproc 输出无法解析为整数")?;
    if cores < 1 {
        bail!("[预检查] CPU 核心数不足: {} 核，需要 >= 1 核", cores);
    }
    Ok(())
}

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

pub async fn check_ulimits(runner: &dyn CommandRunner) -> Result<()> {
    let cmd = "awk '/^Max open files/{print $4} /^Max processes/{print $3}' /proc/self/limits";
    let (out, _) = runner.exec(cmd).await.map_err(|e| anyhow::anyhow!(e))?;
    let text = std::str::from_utf8(&out).unwrap_or("").trim().to_string();
    let mut lines = text.lines();
    let nofile = lines.next().unwrap_or("unlimited").trim().to_string();
    let nproc = lines.next().unwrap_or("unlimited").trim().to_string();

    let nofile_low = is_ulimit_low(&nofile);
    let nproc_low = is_ulimit_low(&nproc);

    if nofile_low || nproc_low {
        if nofile_low {
            ui::check_warn("ulimit nofile", &format!("当前 {}，建议 >= 65536", nofile));
        }
        if nproc_low {
            ui::check_warn("ulimit nproc", &format!("当前 {}，建议 >= 65536", nproc));
        }
    } else {
        ui::check_ok("ulimit", &format!("nofile={} nproc={}", nofile, nproc));
    }
    Ok(())
}

fn is_ulimit_low(val_str: &str) -> bool {
    const MIN: u64 = 65536;
    match val_str.trim().parse::<u64>() {
        Ok(n) => n < MIN,
        Err(_) => false,
    }
}

pub async fn check_selinux(runner: &dyn CommandRunner) -> Result<()> {
    let (out, _) = match runner.exec("getenforce 2>/dev/null || echo absent").await {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let mode = std::str::from_utf8(&out).unwrap_or("").trim();
    match mode {
        "Enforcing" => {
            ui::check_warn("SELinux", "Enforcing 模式，可能阻断 DM 进程");
        }
        "Permissive" => {
            ui::check_ok("SELinux", "Permissive");
        }
        _ => {
            ui::check_ok("SELinux", "已禁用");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockRunner;

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
    async fn test_check_memory_insufficient() {
        let runner = MockRunner::new(vec![(
            "grep '^MemTotal:'".to_string(),
            0,
            mem_output_kb(2 * 1024 * 1024),
        )]);
        let result = check_memory(&runner).await;
        assert!(result.is_err(), "内存不足应返回 Err");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("内存不足"), "应含 '内存不足': {msg}");
        assert!(msg.contains("4 GB"), "应含 '4 GB': {msg}");
    }

    #[tokio::test]
    async fn test_check_cpu_cores_insufficient() {
        let runner = MockRunner::new(vec![("nproc".to_string(), 0, b"0\n".to_vec())]);
        let result = check_cpu_cores(&runner).await;
        assert!(result.is_err(), "0 核应返回 Err");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("CPU 核心数不足"),
            "应含 'CPU 核心数不足': {msg}"
        );
    }

    #[test]
    fn test_parse_memtotal_kb_normal() {
        let input = b"MemTotal:       8388608 kB\n";
        let result = parse_memtotal_kb(input).unwrap();
        assert_eq!(result, 8_388_608);
    }

    #[tokio::test]
    async fn test_check_ulimits_low_nofile_returns_ok() {
        let runner = MockRunner::new(vec![("awk".to_string(), 0, b"512\n65536\n".to_vec())]);
        let result = check_ulimits(&runner).await;
        assert!(
            result.is_ok(),
            "ulimit 偏低应返回 Ok（warn 但不阻断）: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_check_ulimits_all_sufficient_returns_ok() {
        let runner = MockRunner::new(vec![("awk".to_string(), 0, b"65536\n65536\n".to_vec())]);
        assert!(check_ulimits(&runner).await.is_ok());
    }

    #[tokio::test]
    async fn test_check_ulimits_unlimited_returns_ok() {
        let runner = MockRunner::new(vec![(
            "awk".to_string(),
            0,
            b"unlimited\nunlimited\n".to_vec(),
        )]);
        assert!(check_ulimits(&runner).await.is_ok());
    }

    #[tokio::test]
    async fn test_check_selinux_enforcing_returns_ok() {
        let runner = MockRunner::new(vec![("getenforce".to_string(), 0, b"Enforcing\n".to_vec())]);
        let result = check_selinux(&runner).await;
        assert!(
            result.is_ok(),
            "SELinux Enforcing 应 warn 但返回 Ok: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_check_selinux_permissive_returns_ok() {
        let runner = MockRunner::new(vec![(
            "getenforce".to_string(),
            0,
            b"Permissive\n".to_vec(),
        )]);
        assert!(check_selinux(&runner).await.is_ok());
    }

    #[tokio::test]
    async fn test_check_selinux_unavailable_returns_ok() {
        let runner = MockRunner::new_strict(vec![]);
        assert!(
            check_selinux(&runner).await.is_ok(),
            "getenforce 不可用应跳过"
        );
    }
}
