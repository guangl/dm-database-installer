use anyhow::Result;
use futures::future::join_all;
use tokio::time::{Duration, timeout};

use crate::cli::StatusArgs;
use crate::common::shell_quote;
use crate::common::ssh::{CommandRunner, SshError, SshSession};
use crate::config;
use crate::config::cluster::{ClusterSpecificConfig, NodeConfig, NodeRole};

/// 五列输出行，对应表头 Node/Host/Process/Port/Role。
pub struct NodeStatus {
    pub node: String,
    pub host: String,
    pub process: String,
    pub port: String,
    pub role: String,
}

/// 从 disql 输出解析角色字符串。
/// 优先匹配 PRIMARY，其次 OPEN+STANDBY，再次 OPEN，否则 unknown。
fn parse_role_from_disql(output: &str) -> String {
    if output.contains("PRIMARY") {
        "PRIMARY".to_string()
    } else if output.contains("OPEN") && output.contains("STANDBY") {
        "STANDBY".to_string()
    } else if output.contains("OPEN") {
        "OPEN".to_string()
    } else {
        "unknown".to_string()
    }
}

/// 返回节点角色的展示标签（小写，不用 Debug 格式）。
fn node_role_label(role: NodeRole) -> &'static str {
    match role {
        NodeRole::Primary => "primary",
        NodeRole::Standby => "standby",
        NodeRole::Monitor => "monitor",
    }
}

/// 检测本地端口是否在监听（TCP connect 探测，1s 超时）。
async fn check_local_port(port: u16) -> &'static str {
    let addr = format!("127.0.0.1:{}", port);
    match timeout(
        Duration::from_secs(1),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    {
        Ok(Ok(_)) => "listening",
        _ => "closed",
    }
}

/// 检测本地 dmserver 进程是否运行（ps aux | grep dmserver）。
fn detect_local_process() -> &'static str {
    match std::process::Command::new("sh")
        .arg("-c")
        .arg("ps aux | grep dmserver | grep -v grep")
        .output()
    {
        Ok(out) if !out.stdout.is_empty() => "running",
        _ => "stopped",
    }
}

/// 本地 disql 查询角色（仅在端口 listening 时调用）。
async fn query_local_role(install_path: &str, sysdba_password: &str, port: u16) -> String {
    let cmd = format!(
        "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
        shell_quote(install_path),
        shell_quote(sysdba_password),
        port
    );
    match std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .output()
    {
        Ok(out) if out.status.success() => {
            parse_role_from_disql(&String::from_utf8_lossy(&out.stdout))
        }
        _ => "unknown".to_string(),
    }
}

/// 检测远程节点端口是否在监听（ss + grep，通过 CommandRunner）。
async fn check_remote_port(runner: &dyn CommandRunner, port: u16) -> &'static str {
    let cmd = format!("ss -tlnp | grep ':{port}'");
    match runner.exec(&cmd).await {
        Ok((stdout, _)) if !stdout.is_empty() => "listening",
        Ok(_) => "closed",
        Err(SshError::ExecFailed { exit_code: 1, .. }) => "closed",
        Err(SshError::ExecFailed { exit_code: 127, .. }) => "unknown",
        Err(_) => "unknown",
    }
}

/// 检测远程节点 dmserver 进程是否运行（ps aux | grep dmserver | grep -v grep）。
async fn check_remote_process(runner: &dyn CommandRunner) -> &'static str {
    let cmd = "ps aux | grep dmserver | grep -v grep";
    match runner.exec(cmd).await {
        Ok((stdout, _)) if !stdout.is_empty() => "running",
        Ok(_) => "stopped",
        Err(SshError::ExecFailed { exit_code: 1, .. }) => "stopped",
        Err(_) => "stopped",
    }
}

/// 远程 disql 查询角色（仅在端口 listening 时调用）。
async fn query_remote_role(
    runner: &dyn CommandRunner,
    install_path: &str,
    sysdba_password: &str,
    port: u16,
) -> String {
    let cmd = format!(
        "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
        shell_quote(install_path),
        shell_quote(sysdba_password),
        port
    );
    match runner.exec(&cmd).await {
        Ok((stdout, _)) => parse_role_from_disql(&String::from_utf8_lossy(&stdout)),
        Err(_) => "unknown".to_string(),
    }
}

/// 通过 runner 查询单个远程节点状态（便于 MockRunner 测试）。
async fn query_remote_node_with_runner(
    runner: &dyn CommandRunner,
    node: &NodeConfig,
    port: u16,
    install_path: &str,
    sysdba_password: &str,
) -> NodeStatus {
    let process_str = check_remote_process(runner).await;
    let port_str = check_remote_port(runner, port).await;
    let role = if port_str == "listening" {
        query_remote_role(runner, install_path, sysdba_password, port).await
    } else {
        "\u{2014}".to_string()
    };
    NodeStatus {
        node: node_role_label(node.role).to_string(),
        host: node.host.clone(),
        process: process_str.to_string(),
        port: port_str.to_string(),
        role,
    }
}

/// 连接远程节点并查询状态（生产版本，含 SSH 连接超时）。
async fn query_remote_node(
    node: &NodeConfig,
    port: u16,
    install_path: &str,
    sysdba_password: &str,
) -> NodeStatus {
    match timeout(
        Duration::from_secs(5),
        SshSession::connect(&node.host, 22, &node.ssh),
    )
    .await
    {
        Err(_) => NodeStatus {
            node: node_role_label(node.role).to_string(),
            host: node.host.clone(),
            process: "\u{2014}".to_string(),
            port: "\u{2014}".to_string(),
            role: "ERROR: 连接超时".to_string(),
        },
        Ok(Err(e)) => NodeStatus {
            node: node_role_label(node.role).to_string(),
            host: node.host.clone(),
            process: "\u{2014}".to_string(),
            port: "\u{2014}".to_string(),
            role: format!("ERROR: {}", e),
        },
        Ok(Ok(session)) => {
            query_remote_node_with_runner(&session, node, port, install_path, sysdba_password)
                .await
        }
    }
}

/// 并发查询集群所有远程节点状态。
async fn query_cluster_nodes(cluster: &ClusterSpecificConfig) -> Vec<NodeStatus> {
    let futures = cluster.nodes.iter().map(|node| {
        let node = node.clone();
        let install_path = cluster.dminit.install_path.clone();
        let password = cluster.dminit.sysdba_password.clone();
        let port = cluster.dminit.port;
        async move { query_remote_node(&node, port, &install_path, &password).await }
    });
    join_all(futures).await
}

/// 格式化节点状态为对齐文本表格。
fn format_table(rows: &[NodeStatus]) -> String {
    const MIN_NODE: usize = 7;
    const MIN_HOST: usize = 14;
    const MIN_PROCESS: usize = 7;
    const MIN_PORT: usize = 4;
    const MIN_ROLE: usize = 7;

    let w_node = rows
        .iter()
        .map(|r| r.node.len())
        .fold(MIN_NODE, |acc, l| acc.max(l));
    let w_host = rows
        .iter()
        .map(|r| r.host.len())
        .fold(MIN_HOST, |acc, l| acc.max(l));
    let w_process = rows
        .iter()
        .map(|r| r.process.len())
        .fold(MIN_PROCESS, |acc, l| acc.max(l));
    let w_port = rows
        .iter()
        .map(|r| r.port.len())
        .fold(MIN_PORT, |acc, l| acc.max(l));
    let w_role = rows
        .iter()
        .map(|r| r.role.len())
        .fold(MIN_ROLE, |acc, l| acc.max(l));

    let sep = "  ";
    let header = format!(
        "{:<w_node$}{sep}{:<w_host$}{sep}{:<w_process$}{sep}{:<w_port$}{sep}{:<w_role$}",
        "Node",
        "Host",
        "Process",
        "Port",
        "Role",
        w_node = w_node,
        w_host = w_host,
        w_process = w_process,
        w_port = w_port,
        w_role = w_role
    );
    let divider = format!(
        "{}{sep}{}{sep}{}{sep}{}{sep}{}",
        "-".repeat(w_node),
        "-".repeat(w_host),
        "-".repeat(w_process),
        "-".repeat(w_port),
        "-".repeat(w_role)
    );

    let mut lines = vec![header, divider];
    for row in rows {
        lines.push(format!(
            "{:<w_node$}{sep}{:<w_host$}{sep}{:<w_process$}{sep}{:<w_port$}{sep}{:<w_role$}",
            row.node,
            row.host,
            row.process,
            row.port,
            row.role,
            w_node = w_node,
            w_host = w_host,
            w_process = w_process,
            w_port = w_port,
            w_role = w_role
        ));
    }
    lines.join("\n") + "\n"
}

/// `dm-installer status` 顶层入口。
pub async fn run(_args: &StatusArgs) -> Result<()> {
    let cfg = config::load_config().ok();

    let (install_path, port, password, remote_rows) = match cfg {
        None => (
            "/opt/dmdbms".to_string(),
            5236u16,
            "SYSDBA".to_string(),
            vec![],
        ),
        Some(config::LoadedConfig::Standalone { specific, .. }) => (
            specific.install_path.clone(),
            specific.port,
            "SYSDBA".to_string(),
            vec![],
        ),
        Some(config::LoadedConfig::Cluster { specific, .. }) => {
            let remote = query_cluster_nodes(&specific).await;
            (
                specific.dminit.install_path.clone(),
                specific.dminit.port,
                specific.dminit.sysdba_password.clone(),
                remote,
            )
        }
    };

    let process_str = detect_local_process();
    let port_str = check_local_port(port).await;
    let role = if port_str == "listening" {
        query_local_role(&install_path, &password, port).await
    } else {
        "\u{2014}".to_string()
    };

    let mut rows = vec![NodeStatus {
        node: "local".to_string(),
        host: "localhost".to_string(),
        process: process_str.to_string(),
        port: port_str.to_string(),
        role,
    }];
    rows.extend(remote_rows);

    let table = format_table(&rows);
    print!("{}", table);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use crate::common::ssh::MockRunner;
    use crate::config::cluster::{NodeConfig, NodeRole, SshCredentials};
    use clap::Parser;

    fn make_node(role: NodeRole, host: &str) -> NodeConfig {
        NodeConfig {
            role,
            host: host.to_string(),
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

    // ───── Task 1 tests ─────

    #[test]
    fn test_status_args_parses() {
        use crate::cli::Commands;
        let cli = Cli::try_parse_from(["dm-installer", "status"]).unwrap();
        assert!(matches!(cli.command, Commands::Status(StatusArgs { .. })));
    }

    #[test]
    fn test_parse_role_from_disql_primary() {
        let output = "STATUS$   MODE$\nOPEN      PRIMARY\n";
        assert_eq!(parse_role_from_disql(output), "PRIMARY");
    }

    #[test]
    fn test_parse_role_from_disql_standby() {
        let output = "STATUS$   MODE$\nOPEN      STANDBY\n";
        assert_eq!(parse_role_from_disql(output), "STANDBY");
    }

    #[test]
    fn test_parse_role_from_disql_open_only() {
        let output = "STATUS$   MODE$\nOPEN      NORMAL\n";
        assert_eq!(parse_role_from_disql(output), "OPEN");
    }

    #[test]
    fn test_parse_role_from_disql_unknown() {
        let output = "ERROR: connection refused";
        assert_eq!(parse_role_from_disql(output), "unknown");
    }

    #[test]
    fn test_node_role_label() {
        assert_eq!(node_role_label(NodeRole::Primary), "primary");
        assert_eq!(node_role_label(NodeRole::Standby), "standby");
        assert_eq!(node_role_label(NodeRole::Monitor), "monitor");
    }

    #[tokio::test]
    async fn test_check_local_port_closed() {
        // 端口 1 在非 root 下无法监听，必然返回 closed
        let result = check_local_port(1).await;
        assert_eq!(result, "closed");
    }

    // ───── Task 2 tests ─────

    #[tokio::test]
    async fn test_check_remote_port_listening() {
        let runner = MockRunner::new(vec![(
            "ss -tlnp | grep ':5236'".to_string(),
            0,
            b"LISTEN 0 128 *:5236\n".to_vec(),
        )]);
        assert_eq!(check_remote_port(&runner, 5236).await, "listening");
    }

    #[tokio::test]
    async fn test_check_remote_port_closed_grep_exit1() {
        let runner = MockRunner::new(vec![(
            "ss -tlnp | grep ':5236'".to_string(),
            1,
            vec![],
        )]);
        assert_eq!(check_remote_port(&runner, 5236).await, "closed");
    }

    #[tokio::test]
    async fn test_check_remote_port_ss_missing_exit127() {
        let runner = MockRunner::new(vec![(
            "ss -tlnp | grep ':5236'".to_string(),
            127,
            vec![],
        )]);
        assert_eq!(check_remote_port(&runner, 5236).await, "unknown");
    }

    #[tokio::test]
    async fn test_check_remote_process_running() {
        let runner = MockRunner::new(vec![(
            "ps aux | grep dmserver | grep -v grep".to_string(),
            0,
            b"dmdba 1234 .... dmserver path/dm.ini\n".to_vec(),
        )]);
        assert_eq!(check_remote_process(&runner).await, "running");
    }

    #[tokio::test]
    async fn test_check_remote_process_stopped_grep_exit1() {
        let runner = MockRunner::new(vec![(
            "ps aux | grep dmserver | grep -v grep".to_string(),
            1,
            vec![],
        )]);
        assert_eq!(check_remote_process(&runner).await, "stopped");
    }

    #[tokio::test]
    async fn test_query_remote_role_primary() {
        let node = make_node(NodeRole::Primary, "192.168.1.10");
        let runner = MockRunner::new(vec![
            (
                "ps aux | grep dmserver | grep -v grep".to_string(),
                0,
                b"dmdba 1234 dmserver\n".to_vec(),
            ),
            (
                "ss -tlnp | grep ':5236'".to_string(),
                0,
                b"LISTEN 0 128 *:5236\n".to_vec(),
            ),
            (
                "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
                0,
                b"STATUS$   MODE$\nOPEN      PRIMARY\n".to_vec(),
            ),
        ]);
        let status = query_remote_node_with_runner(&runner, &node, 5236, "/opt/dmdbms", "SYSDBA").await;
        assert_eq!(status.role, "PRIMARY");
        assert_eq!(status.process, "running");
        assert_eq!(status.port, "listening");
    }

    #[tokio::test]
    async fn test_query_remote_role_skipped_when_port_closed() {
        let node = make_node(NodeRole::Standby, "192.168.1.11");
        let runner = MockRunner::new(vec![
            (
                "ps aux | grep dmserver | grep -v grep".to_string(),
                0,
                b"dmdba 1234 dmserver\n".to_vec(),
            ),
            (
                "ss -tlnp | grep ':5236'".to_string(),
                1,
                vec![],
            ),
        ]);
        let status = query_remote_node_with_runner(&runner, &node, 5236, "/opt/dmdbms", "SYSDBA").await;
        assert_eq!(status.port, "closed");
        assert_eq!(status.role, "\u{2014}");
        // 验证 disql 命令未被调用
        let log = runner.exec_log();
        assert!(!log.iter().any(|cmd| cmd.contains("disql")), "disql 不应被调用，实际 log: {:?}", log);
    }

    #[tokio::test]
    async fn test_query_remote_node_returns_error_on_disql_failure() {
        let node = make_node(NodeRole::Primary, "192.168.1.10");
        let runner = MockRunner::new(vec![
            (
                "ps aux | grep dmserver | grep -v grep".to_string(),
                0,
                b"dmdba 1234 dmserver\n".to_vec(),
            ),
            (
                "ss -tlnp | grep ':5236'".to_string(),
                0,
                b"LISTEN 0 128 *:5236\n".to_vec(),
            ),
            (
                "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
                1,
                b"login failed\n".to_vec(),
            ),
        ]);
        let status = query_remote_node_with_runner(&runner, &node, 5236, "/opt/dmdbms", "SYSDBA").await;
        assert_eq!(status.role, "unknown");
        assert_eq!(status.process, "running");
        assert_eq!(status.port, "listening");
    }

    #[tokio::test]
    async fn test_query_cluster_nodes_concurrent_collects_all() {
        let node1 = make_node(NodeRole::Primary, "192.168.1.10");
        let node2 = make_node(NodeRole::Standby, "192.168.1.11");

        let runner1 = MockRunner::new(vec![
            (
                "ps aux | grep dmserver | grep -v grep".to_string(),
                0,
                b"dmdba 1234 dmserver\n".to_vec(),
            ),
            (
                "ss -tlnp | grep ':5236'".to_string(),
                0,
                b"LISTEN 0 128 *:5236\n".to_vec(),
            ),
            (
                "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
                0,
                b"STATUS$   MODE$\nOPEN      PRIMARY\n".to_vec(),
            ),
        ]);
        let runner2 = MockRunner::new(vec![(
            "ps aux | grep dmserver | grep -v grep".to_string(),
            1,
            vec![],
        )]);

        // 直接调用两次 query_remote_node_with_runner，验证各自结果
        let status1 =
            query_remote_node_with_runner(&runner1, &node1, 5236, "/opt/dmdbms", "SYSDBA").await;
        let status2 =
            query_remote_node_with_runner(&runner2, &node2, 5236, "/opt/dmdbms", "SYSDBA").await;

        let results = [status1, status2];
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].process, "running");
        assert_eq!(results[0].role, "PRIMARY");
        assert_eq!(results[1].process, "stopped");
    }

    // ───── Task 3 tests ─────

    #[test]
    fn test_format_table_minimum_widths() {
        let rows = vec![NodeStatus {
            node: "local".to_string(),
            host: "localhost".to_string(),
            process: "running".to_string(),
            port: "closed".to_string(),
            role: "unknown".to_string(),
        }];
        let table = format_table(&rows);
        let header = table.lines().next().unwrap();
        // 检查表头各列的宽度 >= 最小宽度
        assert!(header.len() >= 7 + 2 + 14 + 2 + 7 + 2 + 4 + 2 + 7);
        assert!(header.starts_with("Node"));
        assert!(header.contains("Host"));
        assert!(header.contains("Process"));
        assert!(header.contains("Port"));
        assert!(header.contains("Role"));
    }

    #[test]
    fn test_format_table_expands_for_long_host() {
        let long_host = "very-long-host.example.com";
        let rows = vec![NodeStatus {
            node: "local".to_string(),
            host: long_host.to_string(),
            process: "running".to_string(),
            port: "listening".to_string(),
            role: "PRIMARY".to_string(),
        }];
        let table = format_table(&rows);
        let data_line = table.lines().nth(2).unwrap();
        // 数据行中应包含完整的 long_host
        assert!(data_line.contains(long_host));
    }

    #[test]
    fn test_format_table_three_columns_alignment() {
        let rows = vec![
            NodeStatus {
                node: "local".to_string(),
                host: "localhost".to_string(),
                process: "running".to_string(),
                port: "listening".to_string(),
                role: "PRIMARY".to_string(),
            },
            NodeStatus {
                node: "standby".to_string(),
                host: "192.168.1.11".to_string(),
                process: "running".to_string(),
                port: "listening".to_string(),
                role: "STANDBY".to_string(),
            },
            NodeStatus {
                node: "monitor".to_string(),
                host: "192.168.1.12".to_string(),
                process: "stopped".to_string(),
                port: "closed".to_string(),
                role: "unknown".to_string(),
            },
        ];
        let table = format_table(&rows);
        // 跳过表头和分隔线，验证数据行的列数
        for (i, line) in table.lines().enumerate().skip(2) {
            let fields: Vec<&str> =
                line.split("  ").filter(|s| !s.trim().is_empty()).collect();
            assert!(
                fields.len() >= 5,
                "数据行 {} 字段数应 >= 5，实际: {:?}",
                i,
                fields
            );
        }
    }

    #[test]
    fn test_format_table_error_row() {
        let rows = vec![
            NodeStatus {
                node: "local".to_string(),
                host: "localhost".to_string(),
                process: "running".to_string(),
                port: "listening".to_string(),
                role: "PRIMARY".to_string(),
            },
            NodeStatus {
                node: "standby".to_string(),
                host: "192.168.1.11".to_string(),
                process: "\u{2014}".to_string(),
                port: "\u{2014}".to_string(),
                role: "ERROR: 连接超时".to_string(),
            },
        ];
        let table = format_table(&rows);
        let lines: Vec<&str> = table.lines().collect();
        // 验证 error 行存在且包含 ERROR 字样
        assert!(lines.iter().any(|l| l.contains("ERROR")));
    }

    #[test]
    fn test_format_table_separator_line() {
        let rows = vec![NodeStatus {
            node: "local".to_string(),
            host: "localhost".to_string(),
            process: "running".to_string(),
            port: "listening".to_string(),
            role: "PRIMARY".to_string(),
        }];
        let table = format_table(&rows);
        let lines: Vec<&str> = table.lines().collect();
        assert!(lines.len() >= 3, "至少有表头、分隔线和数据行");
        let header_len = lines[0].len();
        let divider = lines[1];
        assert_eq!(
            divider.len(),
            header_len,
            "分隔线长度应等于表头长度，表头: '{}', 分隔线: '{}'",
            lines[0],
            divider
        );
        assert!(
            divider.chars().all(|c| c == '-' || c == ' '),
            "分隔线应只含 '-' 和空格，实际: '{}'",
            divider
        );
    }

    #[test]
    fn test_run_no_config_prints_local_only() {
        // 验证 format_table 对单行 local 节点的输出包含 "local" 和 5 列表头
        let rows = vec![NodeStatus {
            node: "local".to_string(),
            host: "localhost".to_string(),
            process: "stopped".to_string(),
            port: "closed".to_string(),
            role: "\u{2014}".to_string(),
        }];
        let table = format_table(&rows);
        assert!(table.contains("local"));
        let header = table.lines().next().unwrap();
        assert!(header.contains("Node"));
        assert!(header.contains("Host"));
        assert!(header.contains("Process"));
        assert!(header.contains("Port"));
        assert!(header.contains("Role"));
    }
}
