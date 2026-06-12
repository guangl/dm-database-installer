use anyhow::{anyhow, Result};
use tokio::net::TcpStream;
use tokio::time::{Duration, sleep, timeout};

/// TCP 轮询间隔（D-09）。
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// 等待目标 host:port 的 TCP 连接就绪，超出 max_secs 则返回 Err。
pub async fn wait_tcp_ready(host: &str, port: u16, max_secs: u64) -> Result<()> {
    let addr = format!("{}:{}", host, port);
    let result = timeout(
        Duration::from_secs(max_secs),
        poll_loop(&addr, POLL_INTERVAL),
    )
    .await;
    result.map_err(|_| anyhow!("主节点 {} 在 {}s 内未就绪", addr, max_secs))
}

/// 持续轮询 TCP 连接直到成功。
async fn poll_loop(addr: &str, interval: Duration) {
    loop {
        match TcpStream::connect(addr).await {
            Ok(_) => return,
            Err(_) => sleep(interval).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn test_wait_tcp_ready_times_out() {
        // 端口 1 在非 root 下无法监听，必然超时
        let start = Instant::now();
        let result = wait_tcp_ready("127.0.0.1", 1, 2).await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "应超时返回 Err");
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("127.0.0.1:1"), "错误消息应含 host:port: {msg}");
        assert!(msg.contains("2s"), "错误消息应含超时秒数: {msg}");
        assert!(
            elapsed >= Duration::from_millis(1500),
            "耗时应 >= 1.5s，实际 {:?}",
            elapsed
        );
        assert!(
            elapsed <= Duration::from_millis(3500),
            "耗时应 <= 3.5s，实际 {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_wait_tcp_ready_immediate_success() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let start = Instant::now();
        let result = wait_tcp_ready("127.0.0.1", port, 5).await;
        assert!(result.is_ok(), "立即就绪应返回 Ok: {:?}", result.err());
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "首次连接应 < 500ms，实际 {:?}",
            start.elapsed()
        );
    }
}
