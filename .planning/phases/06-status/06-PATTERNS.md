# Phase 6: status 命令 - Pattern Map

**Mapped:** 2026-06-14
**Files analyzed:** 3 (new/modified files)
**Analogs found:** 3 / 3

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/status/mod.rs` | service + utility | request-response + event-driven | `src/cluster/preflight.rs` | role-match (并发查询 + 错误收集模式完全一致) |
| `src/cli.rs` | CLI config | request-response | `src/cli.rs` 自身 (ValidateArgs 模式) | exact |
| `src/main.rs` | entry point | request-response | `src/main.rs` 自身 (dispatch 模式) | exact |

## Pattern Assignments

### `src/cli.rs` — 新增 `Status(StatusArgs)` 变体

**Analog:** `src/cli.rs` 现有 `Validate(ValidateArgs)` 模式

**现有 Commands 枚举结构** (lines 17-30):
```rust
#[derive(Subcommand)]
pub enum Commands {
    Install(InstallArgs),
    Validate(ValidateArgs),
    Init(InitArgs),
    Completions {
        shell: clap_complete::Shell,
    },
}
```

**新增变体参照 ValidateArgs 风格** (lines 49-53):
```rust
/// validate 子命令参数
#[derive(clap::Args)]
pub struct ValidateArgs {
    /// 配置文件路径（默认读取当前目录 config.toml）
    pub config: Option<PathBuf>,
}
```

**StatusArgs 应实现为（D-01 决策：无必填参数）:**
```rust
/// status 子命令参数
#[derive(clap::Args)]
pub struct StatusArgs {
    // 无必填参数；config.toml 自动从当前目录发现
}
```

**CLI 测试模式** (lines 87-119) — 新增 test 与 `test_validate_defaults_to_no_path` 对称:
```rust
#[test]
fn test_validate_defaults_to_no_path() {
    let cli = Cli::try_parse_from(["dm-installer", "validate"]).unwrap();
    let Commands::Validate(args) = cli.command else { panic!("expected Validate") };
    assert!(args.config.is_none());
}
```

---

### `src/main.rs` — 新增 `mod status` + dispatch 分支

**Analog:** `src/main.rs` 现有 dispatch 结构

**模块声明模式** (lines 6-11):
```rust
mod cli;
mod cluster;
mod common;
mod config;
mod guide;
mod standalone;
// 新增：mod status;
```

**Dispatch 模式** (lines 20-45):
```rust
match &cli_args.command {
    cli::Commands::Install(args) => {
        let cfg = config::load_config().unwrap_or_else(|e| {
            eprintln!("{e}");
            guide::print_install();
            std::process::exit(1);
        });
        // ...
    }
    cli::Commands::Validate(args) => config::validate::run(args).await,
    cli::Commands::Init(args) => config::init::run(&args.kind),
    // 新增分支：
    // cli::Commands::Status(args) => status::run(args).await,
}
```

**关键差异：** Status 不同于 Install——config 缺失时不报错不退出，直接进入 no-config 本地模式（D-04）。不能复用 `unwrap_or_else + std::process::exit(1)` 的 Install 模式。

---

### `src/status/mod.rs` — 新建，整个 status 功能

这是本 phase 唯一全新文件，约 200 行，无已有同类文件可完全对照。以下逐功能块给出最接近的代码来源。

#### 1. 导入模式 — 参照 `src/cluster/preflight.rs` (lines 1-8)

```rust
use anyhow::Result;
use futures::future::join_all;

use crate::common::ssh::CommandRunner;
use crate::config::cluster::{ClusterSpecificConfig, DminitConfig, NodeConfig};
```

**status/mod.rs 应添加：**
```rust
use anyhow::Result;
use futures::future::join_all;
use tokio::time::{timeout, Duration};

use crate::cli::StatusArgs;
use crate::common::ssh::{CommandRunner, SshError};
use crate::common::shell_quote;
use crate::config;
use crate::config::cluster::{ClusterSpecificConfig, NodeConfig, NodeRole};
```

#### 2. 配置发现（no-config 不报错）— 参照 `src/guide.rs` 无侵入风格

guide.rs 的核心思路：config 缺失时做最小提示，不 panic。status 命令比 guide.rs 更进一步——不提示、直接降级到本地模式。

**参照 `src/config/mod.rs` `load_config()` 函数（lines 143-146）:**
```rust
pub fn load_config() -> Result<LoadedConfig> {
    load_config_from(Path::new(CONFIG_FILE))
}
```

**status 模块应用模式（D-04/D-05）：**
```rust
pub async fn run(_args: &StatusArgs) -> Result<()> {
    let config_opt = config::load_config().ok(); // 忽略错误，None = no-config 模式
    match config_opt {
        None => {
            // 仅本地状态
        }
        Some(config::LoadedConfig::Standalone { specific, .. }) => {
            // 仅本地，port 来自 specific.port
        }
        Some(config::LoadedConfig::Cluster { specific, .. }) => {
            // 本地 + 所有远程节点
        }
    }
}
```

#### 3. 并发多节点查询 — 参照 `src/cluster/preflight.rs:100-127`

这是最精确的类比：preflight 也是并发查询所有节点并收集结果，不因单节点失败中止。

```rust
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
    // 收集失败，不 early-exit
    let failures: Vec<String> = results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| { ... })
        .collect();
}
```

**status 并发模式应对应：**
```rust
let futures = cluster.nodes.iter().map(|node| {
    let node = node.clone();
    let port = cluster.dminit.port;
    let install_path = cluster.dminit.install_path.clone();
    let sysdba_password = cluster.dminit.sysdba_password.clone();
    async move { query_remote_node(&node, port, &install_path, &sysdba_password).await }
});
let node_statuses: Vec<NodeStatus> = join_all(futures).await;
```

**关键差异（D-15）：** preflight 在所有失败后统一 bail!，status 的 `query_remote_node` 返回的是 `NodeStatus`（含 error 字段）而非 `Result<()>`，永不传播错误。

#### 4. SSH 连接超时 — 参照 `src/cluster/health.rs:9-17`

```rust
pub async fn wait_tcp_ready(host: &str, port: u16, max_secs: u64) -> Result<()> {
    let addr = format!("{}:{}", host, port);
    let result = timeout(
        Duration::from_secs(max_secs),
        poll_loop(&addr, POLL_INTERVAL),
    )
    .await;
    result.map_err(|_| anyhow!("主节点 {} 在 {}s 内未就绪", addr, max_secs))
}
```

**status SSH 连接超时应用模式（D-09 + Pitfall 3）：**
```rust
async fn query_remote_node(node: &NodeConfig, port: u16, install_path: &str, password: &str) -> NodeStatus {
    use crate::common::ssh::SshSession;
    let connect_result = timeout(
        Duration::from_secs(5),
        SshSession::connect(&node.host, 22, &node.ssh),
    ).await;
    match connect_result {
        Err(_) => return NodeStatus::error(node, "连接超时"),
        Ok(Err(e)) => return NodeStatus::error(node, &e.to_string()),
        Ok(Ok(session)) => { /* 继续 */ }
    }
}
```

#### 5. 端口检测（ss + grep exit_code 1 处理）— 参照 `src/cluster/preflight.rs:23-41`

这是最直接的模式，status 远程端口检测完全复用此模式：

```rust
pub async fn check_port_available(runner: &dyn CommandRunner, port: u16) -> Result<()> {
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
```

**status 远程端口检测应返回状态字符串而非 Result：**
```rust
async fn check_remote_port(runner: &dyn CommandRunner, port: u16) -> &'static str {
    let cmd = format!("ss -tlnp | grep ':{port}'");
    match runner.exec(&cmd).await {
        Ok((stdout, _)) if !stdout.is_empty() => "listening",
        Ok(_) => "closed",
        Err(SshError::ExecFailed { exit_code: 1, .. }) => "closed",   // grep 无匹配
        Err(SshError::ExecFailed { exit_code: 127, .. }) => "unknown", // ss 命令不存在
        Err(_) => "unknown",
    }
}
```

#### 6. 本地 TCP 端口检测 — 参照 `src/cluster/health.rs:20-27`

health.rs 的 `poll_loop` 用 `TcpStream::connect` 做轮询，status 只需单次检测：

```rust
async fn poll_loop(addr: &str, interval: Duration) {
    loop {
        match TcpStream::connect(addr).await {
            Ok(_) => return,
            Err(_) => sleep(interval).await,
        }
    }
}
```

**status 本地端口检测（单次，D-07）：**
```rust
async fn check_local_port(port: u16) -> &'static str {
    let addr = format!("127.0.0.1:{}", port);
    match timeout(Duration::from_secs(1), tokio::net::TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => "listening",
        _ => "closed",
    }
}
```

#### 7. disql 命令构造和输出解析 — 参照 `src/cluster/phases.rs:276-306`

```rust
// phases.rs lines 276-282: disql 命令格式（VERIFIED）
use crate::common::shell_quote;
let cmd = format!(
    "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
    shell_quote(&dminit.install_path),
    shell_quote(&dminit.sysdba_password),
    dminit.port,
);

// phases.rs lines 289-291: 输出解析模式（VERIFIED）
let output = String::from_utf8_lossy(&stdout);
if output.contains("OPEN") && output.contains("STANDBY") {
    // 备节点就绪
}
```

**status 角色解析扩展（Claude's Discretion 范围）：**
```rust
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
```

**关键注意：** phases.rs 对 disql exit_code 非 0 用 `?` 传播；status 必须静默处理（Role 显示 `unknown`），否则违反 D-15。

#### 8. MockRunner 测试模式 — 参照 `src/common/ssh/mock.rs` + `src/cluster/preflight.rs:129-257`

MockRunner 构建（preflight 测试 lines 173-182）：
```rust
let runner = MockRunner::new(vec![
    ("sudo -n true".to_string(), 0, vec![]),
    ("ss -tlnp | grep ':5236'".to_string(), 1, vec![]),  // exit 1 = 端口空闲
    ("df -B1 /opt".to_string(), 0, df_out),
]);
let result = check_node(&node, &make_dminit(), &runner).await;
assert!(result.is_ok(), "三项全通过应返回 Ok: {:?}", result.err());
```

**status 测试应用相同框架：**
```rust
// 测试 disql 解析 PRIMARY
let runner = MockRunner::new(vec![
    ("ps aux | grep dmserver".to_string(), 0, b"dmserver ...\n".to_vec()),
    ("ss -tlnp | grep ':5236'".to_string(), 0, b"LISTEN 0 128 *:5236\n".to_vec()),
    ("echo 'SELECT STATUS$".to_string(), 0, b"STATUS$   MODE$\nOPEN      PRIMARY\n".to_vec()),
]);
```

---

## Shared Patterns

### grep exit_code 1 处理
**Source:** `src/cluster/preflight.rs:35`
**Apply to:** `src/status/mod.rs` 中所有使用 `ss | grep` 的端口检测函数

```rust
Err(crate::common::ssh::SshError::ExecFailed { exit_code: 1, .. }) => {
    // grep 无匹配 = 正常情况，视为端口未监听
    Ok(())
}
```

### shell_quote 参数转义
**Source:** `src/common/mod.rs` 中的 `shell_quote` 函数（被 `src/cluster/phases.rs:276` 引用）
**Apply to:** `src/status/mod.rs` 中构造 disql 命令时的 install_path 和 sysdba_password

```rust
use crate::common::shell_quote;
let cmd = format!(
    "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
    shell_quote(install_path),
    shell_quote(sysdba_password),
    port,
);
```

### tokio::time::timeout 超时控制
**Source:** `src/cluster/health.rs:11-16`
**Apply to:** `src/status/mod.rs` 中所有 SSH 连接和远程命令执行

```rust
use tokio::time::{timeout, Duration};
let result = timeout(Duration::from_secs(5), async_operation).await;
```

### join_all 并发收集（不因单节点失败中止）
**Source:** `src/cluster/preflight.rs:106-121`
**Apply to:** `src/status/mod.rs` 中的远程节点并发查询

```rust
use futures::future::join_all;
let results: Vec<NodeStatus> = join_all(futures).await;
// 注意：join_all 不传播错误（不同于 try_join_all），所有错误在 NodeStatus 内部处理
```

### NodeRole 转显示字符串
**Source:** `src/config/cluster.rs:10-17`（`NodeRole` 枚举定义，用 `#[serde(rename_all = "lowercase")]`）

```rust
#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    Primary,
    Standby,
    Monitor,
}
```

**不要使用 `Debug` 输出（输出 `Primary` 而非期望格式），应实现辅助函数：**
```rust
fn node_role_label(role: NodeRole) -> &'static str {
    match role {
        NodeRole::Primary => "primary",
        NodeRole::Standby => "standby",
        NodeRole::Monitor => "monitor",
    }
}
```

---

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `src/status/mod.rs` 中的表格格式化逻辑 | utility | transform | 项目中无手动对齐文本表格的既有实现，D-16 锁定无额外 crate，参照 RESEARCH.md Pattern 5 的假设示例 |
| `src/status/mod.rs` 中的本地进程检测 | utility | request-response | `std::process::Command` 用于本地进程的模式在项目中仅在 standalone 中存在（非相关用途），status 是首个用 ps aux 检测的代码 |

---

## 关键执行细节

### SshSession::connect 签名
参照 `src/common/ssh/session.rs`（CONTEXT.md 引用）：
- 调用方式：`SshSession::connect(host, 22, &node.ssh)` 返回 `Result<SshSession>`
- 22 是硬编码（CR-01 已知问题，Phase 6 不修复）

### LoadedConfig 枚举
参照 `src/config/mod.rs:131-141`：
```rust
pub enum LoadedConfig {
    Standalone {
        common: CommonConfig,
        specific: InstallConfig,   // port 字段直接可用
    },
    Cluster {
        common: CommonConfig,
        specific: cluster::ClusterSpecificConfig,  // .dminit.port, .nodes 字段
        install_type: InstallType,
    },
}
```

### ClusterSpecificConfig 字段
参照 `src/config/cluster.rs:218-244`：
- `specific.nodes: Vec<NodeConfig>` — 所有节点
- `specific.dminit.port: u16` — 数据库端口
- `specific.dminit.install_path: String` — disql 路径基准
- `specific.dminit.sysdba_password: String` — disql 密码

### 本地 InstallConfig 字段
参照 `src/config/mod.rs:260-271`：
- `specific.port: u16` — 数据库端口
- `specific.install_path: String` — disql 路径基准

---

## Metadata

**Analog search scope:** `src/cluster/`, `src/config/`, `src/common/ssh/`, `src/cli.rs`, `src/main.rs`, `src/guide.rs`
**Files scanned:** 10 (cli.rs, main.rs, preflight.rs, phases.rs, health.rs, config/cluster.rs, config/mod.rs, common/ssh/mock.rs, common/ssh/runner.rs, guide.rs)
**Pattern extraction date:** 2026-06-14
