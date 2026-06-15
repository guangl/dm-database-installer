use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use futures::future::join_all;

use crate::common::ssh::CommandRunner;
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

/// 检查安装路径父目录的磁盘剩余空间（要求 >= 5 GB）。
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
    let min_bytes: u64 = 5 * 1024 * 1024 * 1024;
    tracing::debug!(
        "[预检查] 磁盘剩余: {} GB ({} bytes), 最低要求: 5 GB",
        available / (1024 * 1024 * 1024),
        available
    );
    if available < min_bytes {
        bail!(
            "[预检查] 磁盘空间不足: 剩余 {} bytes，需要 >= 5 GB",
            available
        );
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

/// 对单个节点执行全部三项预检查（sudo / 端口 / 磁盘），任一失败即返回带节点信息的 Err。
pub async fn check_node(node: &NodeConfig, dminit: &DminitConfig, runner: &dyn CommandRunner) -> Result<()> {
    check_sudo_nopass(runner)
        .await
        .with_context(|| format!("节点 {} ({:?}) 预检查失败", node.host, node.role))?;
    check_port_available(runner, dminit.port)
        .await
        .with_context(|| format!("节点 {} ({:?}) 预检查失败", node.host, node.role))?;
    check_disk_space(runner, &dminit.install_path)
        .await
        .with_context(|| format!("节点 {} ({:?}) 预检查失败", node.host, node.role))?;
    Ok(())
}

/// 并发对所有节点执行预检查，收集所有失败节点后统一报告。
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

    #[tokio::test]
    async fn test_check_node_all_pass() {
        let df_out = df_output_with_available(10 * 1024 * 1024 * 1024);
        let runner = MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("ss -tlnp | grep ':5236'".to_string(), 1, vec![]),
            ("df -B1 /opt".to_string(), 0, df_out),
        ]);
        let node = make_node();
        let result = check_node(&node, &make_dminit(), &runner).await;
        assert!(result.is_ok(), "三项全通过应返回 Ok: {:?}", result.err());
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
            ("ss -tlnp | grep ':5236'".to_string(), 1, vec![]),
            ("df -B1 /opt".to_string(), 0, df_out),
        ]);
        let node = make_node();
        let result = check_node(&node, &make_dminit(), &runner).await;
        assert!(result.is_err(), "磁盘不足应返回 Err");
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("磁盘空间不足"), "应含磁盘错误: {msg}");
        assert!(msg.contains("5 GB"), "应含 5 GB 字样: {msg}");
    }

    #[tokio::test]
    async fn test_preflight_all_nodes_mixed() {
        let df_out = df_output_with_available(10 * 1024 * 1024 * 1024);
        let runner_ok = Arc::new(MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
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
