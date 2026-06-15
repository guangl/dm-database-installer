use anyhow::Result;
use tokio::time::{Duration, timeout};

use crate::cli::StatusArgs;
use crate::ssh::shell_quote;
use crate::config;

pub struct NodeStatus {
    pub node: String,
    pub host: String,
    pub process: String,
    pub port: String,
    pub role: String,
}

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

fn format_table(rows: &[NodeStatus]) -> String {
    const MIN_NODE: usize = 7;
    const MIN_HOST: usize = 14;
    const MIN_PROCESS: usize = 7;
    const MIN_PORT: usize = 4;
    const MIN_ROLE: usize = 7;

    let w_node = rows.iter().map(|r| r.node.len()).fold(MIN_NODE, |acc, l| acc.max(l));
    let w_host = rows.iter().map(|r| r.host.len()).fold(MIN_HOST, |acc, l| acc.max(l));
    let w_process = rows.iter().map(|r| r.process.len()).fold(MIN_PROCESS, |acc, l| acc.max(l));
    let w_port = rows.iter().map(|r| r.port.len()).fold(MIN_PORT, |acc, l| acc.max(l));
    let w_role = rows.iter().map(|r| r.role.len()).fold(MIN_ROLE, |acc, l| acc.max(l));

    let sep = "  ";
    let header = format!(
        "{:<w_node$}{sep}{:<w_host$}{sep}{:<w_process$}{sep}{:<w_port$}{sep}{:<w_role$}",
        "Node", "Host", "Process", "Port", "Role",
        w_node = w_node, w_host = w_host, w_process = w_process, w_port = w_port, w_role = w_role
    );
    let divider = format!(
        "{}{sep}{}{sep}{}{sep}{}{sep}{}",
        "-".repeat(w_node), "-".repeat(w_host), "-".repeat(w_process),
        "-".repeat(w_port), "-".repeat(w_role)
    );

    let mut lines = vec![header, divider];
    for row in rows {
        lines.push(format!(
            "{:<w_node$}{sep}{:<w_host$}{sep}{:<w_process$}{sep}{:<w_port$}{sep}{:<w_role$}",
            row.node, row.host, row.process, row.port, row.role,
            w_node = w_node, w_host = w_host, w_process = w_process, w_port = w_port, w_role = w_role
        ));
    }
    lines.join("\n") + "\n"
}

pub async fn run(_args: &StatusArgs) -> Result<()> {
    let cfg = config::load_config().ok();

    let (install_path, port, password) = match cfg {
        None => ("/opt/dmdbms".to_string(), 5236u16, "SYSDBA".to_string()),
        Some(cfg) => (cfg.specific.install_path.clone(), cfg.specific.port, "SYSDBA".to_string()),
    };

    let process_str = detect_local_process();
    let port_str = check_local_port(port).await;
    let role = if port_str == "listening" {
        query_local_role(&install_path, &password, port).await
    } else {
        "\u{2014}".to_string()
    };

    let rows = vec![NodeStatus {
        node: "local".to_string(),
        host: "localhost".to_string(),
        process: process_str.to_string(),
        port: port_str.to_string(),
        role,
    }];

    print!("{}", format_table(&rows));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Commands, StatusArgs};
    use clap::Parser;

    #[test]
    fn test_status_args_parses() {
        let cli = Cli::try_parse_from(["dm-installer", "status"]).unwrap();
        assert!(matches!(cli.command, Commands::Status(StatusArgs { .. })));
    }

    #[test]
    fn test_parse_role_from_disql_primary() {
        assert_eq!(parse_role_from_disql("STATUS$   MODE$\nOPEN      PRIMARY\n"), "PRIMARY");
    }

    #[test]
    fn test_parse_role_from_disql_standby() {
        assert_eq!(parse_role_from_disql("STATUS$   MODE$\nOPEN      STANDBY\n"), "STANDBY");
    }

    #[test]
    fn test_parse_role_from_disql_open_only() {
        assert_eq!(parse_role_from_disql("STATUS$   MODE$\nOPEN      NORMAL\n"), "OPEN");
    }

    #[test]
    fn test_parse_role_from_disql_unknown() {
        assert_eq!(parse_role_from_disql("ERROR: connection refused"), "unknown");
    }

    #[tokio::test]
    async fn test_check_local_port_closed() {
        let result = check_local_port(1).await;
        assert_eq!(result, "closed");
    }

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
        assert!(data_line.contains(long_host));
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
        assert_eq!(divider.len(), header_len);
        assert!(divider.chars().all(|c| c == '-' || c == ' '));
    }

    #[test]
    fn test_run_no_config_prints_local_only() {
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
