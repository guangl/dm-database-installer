# Phase 6: status 命令 - Research

**Researched:** 2026-06-14
**Domain:** Rust CLI 子命令 / SSH 并发查询 / 终端表格输出
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**CLI 结构**
- D-01: 在 `src/cli.rs` 添加 `Status(StatusArgs)` 变体到 `Commands` 枚举；`StatusArgs` 无必填参数（config.toml 自动发现）。
- D-02: 在 `src/main.rs` 添加 `cli::Commands::Status(args) => status::run(args).await` dispatch 分支。
- D-03: 新建模块 `src/status/mod.rs`，在 `main.rs` 中声明 `mod status`。

**配置发现（No-config 行为）**
- D-04: `dm-installer status` 在当前目录未找到 config.toml 时**不报错**——仅显示本地 DM 实例状态（进程 + 端口），与 `guide.rs` 的无侵入风格一致。
- D-05: 找到 config.toml 时，用现有 `config::load_config()` 加载；若为 `LoadedConfig::Cluster`，SSH 查询所有节点；若为 `LoadedConfig::Standalone`，仅显示本地状态（standalone 无远程节点列表）。

**本地实例检测**
- D-06: 本地进程检测：`std::process::Command::new("sh").arg("-c").arg("ps aux | grep dmserver | grep -v grep")` — 输出非空则 `running`，否则 `stopped`。
- D-07: 本地端口检测：TCP connect 到 `127.0.0.1:PORT`（PORT 来自 config.toml 或默认 5236）；连通则 `listening`，否则 `closed`。
- D-08: 本地角色查询：仅当端口 listening 时尝试 `disql SYSDBA/SYSDBA@localhost:PORT "SELECT STATUS$,MODE$ FROM V$INSTANCE;"`；若失败则 Role 显示 `unknown`。

**远程节点查询**
- D-09: 使用 `SshSession::connect(host, 22, &node.ssh)` 建立连接，复用现有 SSH 基础设施。
- D-10: 远程进程检测：`ps aux | grep dmserver | grep -v grep`（与 D-06 本地方法对称）。
- D-11: 远程端口检测：`ss -tlnp | grep ':{PORT}'`（与 `preflight.rs` 现有模式一致）。
- D-12: 远程角色查询：仅当端口 listening 时执行 `disql SYSDBA/SYSDBA@localhost:{PORT} "SELECT STATUS$,MODE$ FROM V$INSTANCE;"`；解析输出中 `PRIMARY`/`STANDBY`/`OPEN` 关键词。
- D-13: 凭据来源：延用现有代码库的 `SYSDBA/SYSDBA` 硬编码模式（CR-02 是已知问题，Phase 6 不修复）。

**并发与错误处理**
- D-14: 所有远程节点并发查询：`tokio::join_all`（UX 更好，与集群部署并发模式一致）。
- D-15: 单个节点 SSH 连接失败时：该行 Process/Port/Role 列均显示 `—`，Role 列改为 `ERROR: {原因}`；其余节点正常输出，整体命令退出码 0（非致命错误）。

**输出格式**
- D-16: 输出格式：手动对齐文本表格，无额外 crate 依赖。固定列顺序：`Node | Host | Process | Port | Role`。
- D-17: 表头与分隔线示例：
  ```
  Node     Host            Process  Port  Role
  -------  --------------  -------  ----  -------
  local    localhost       running  open  PRIMARY
  standby  192.168.1.101   running  open  STANDBY
  node2    192.168.1.102   stopped  —     —
  ```
- D-18: 节点名称（Node 列）来源：cluster config 的节点 `role` 字段（Primary/Standby）；本地节点固定显示 `local`。

### Claude's Discretion

- 角色字符串解析逻辑（从 disql 输出中提取 STATUS$/MODE$ 值的具体正则/字符串匹配）
- 表格列宽动态计算还是固定宽度（根据最长主机名动态调整更美观）
- 超时参数：SSH 连接超时、disql 查询超时（建议各 5s）

### Deferred Ideas (OUT OF SCOPE)

- 修复 SSH 端口硬编码（CR-01）——NodeConfig 需增加 ssh_port 字段，留 Phase 7 或专项 fix
- 修复 SYSDBA 密码硬编码（CR-02）——config.toml 增加 db_password 字段，留后续 phase
- `--watch` 模式（持续刷新状态表格）——超出本 phase 范围
- JSON 输出格式（`--format json`）——可选增强，本 phase 不做
- Windows 本地进程检测——需 `tasklist | findstr dmserver`，留多平台扩展时处理
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| STAT-01 | 用户可执行 `dm-installer status` 查询本地 DM 实例进程状态与端口监听 | D-06 本地 ps 检测 + D-07 TCP connect 端口检测 |
| STAT-02 | status 命令读取 config.toml 节点列表，通过 SSH 查询所有远程节点状态 | D-04/D-05 配置发现 + D-09~D-12 SSH 查询链 |
| STAT-03 | 状态输出包含每个节点的进程状态、端口监听、数据库角色，格式为对齐表格 | D-16~D-18 输出格式 + disql V$INSTANCE 角色解析 |
</phase_requirements>

## Summary

Phase 6 是一个纯 Rust 模块扩展，无新增外部依赖。所有核心基础设施（SSH 会话、CommandRunner trait、config 加载、futures 并发）已在前几个 phase 中建立，本 phase 直接复用。

新增的 `src/status/mod.rs` 是唯一新文件。主要工作分为三块：（1）本地状态检测（进程 + TCP connect + 本地 disql）；（2）远程节点并发 SSH 查询（进程 + ss + 远程 disql）；（3）对齐表格格式化输出。技术风险低，但有两个需要注意的执行细节：disql 输出格式解析、`ss` 命令在某些 Linux 发行版不存在（iproute2 包）。

**Primary recommendation:** 复用 `MockRunner` 为 status 逻辑写单元测试，尤其是 disql 输出解析和错误行格式。

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| 本地进程检测 | CLI 进程层（std::process） | — | ps aux 是本地 OS 命令，无需 SSH |
| 本地端口检测 | 网络层（TcpStream::connect） | — | tokio::net::TcpStream 单次 connect，直接复用 health.rs 思路 |
| 本地角色查询 | CLI 进程层（std::process） | — | 本地 disql 通过 localhost 连接 |
| 远程状态查询 | SSH 层（CommandRunner::exec） | 并发层（futures::join_all） | 所有远程命令走现有 SshSession::exec |
| 配置发现 | 配置层（config::load_config） | — | 现有 load_config 返回 LoadedConfig，按类型分支 |
| 表格格式化 | CLI 输出层（std::io::stdout） | — | 纯字符串格式化，无 crate 依赖 |

## Standard Stack

### Core（已在 Cargo.toml 中，无需新增）

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `tokio` | 1.52.3 | async runtime + TcpStream | 已有，health.rs TCP connect 思路复用 |
| `futures` | 0.3 | `join_all` 并发多节点查询 | 已有，preflight.rs 用 `join_all` |
| `anyhow` | 1.0.102 | 错误链 | 已有，整个项目统一 |
| `tracing` | 0.1.44 | debug/warn 日志 | 已有 |

### Supporting（已有，按需使用）

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `russh` + `SshSession` | 0.61.2 | SSH 连接 | 远程节点查询 |
| `tokio::time::timeout` | （tokio 内置） | SSH 连接 + disql 查询超时 | 防止挂起 |

**无新依赖需要安装。**

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| 手动对齐表格 | `tabled` crate | tabled 更灵活，但 D-16 已锁定无额外 crate |
| `ps aux \| grep` 进程检测 | `/proc` 文件系统解析 | /proc 只有 Linux，且实现复杂；`ps aux` 命令更通用 |
| TCP connect 端口检测 | `ss` 命令 | TCP connect 结果更直接（可读性确实监听），不依赖 iproute2 包 |

## Package Legitimacy Audit

本 phase 无新外部包引入，跳过此检查。

## Architecture Patterns

### System Architecture Diagram

```
dm-installer status
      │
      ▼
 main.rs dispatch
      │
      ▼
 status::run(args)
      │
      ├── config::load_config() ──► 文件不存在 ──► 仅本地模式
      │         │
      │         ▼
      │    LoadedConfig
      │    ├── Standalone ──► 仅本地模式
      │    └── Cluster ──────► 本地 + 所有远程节点
      │
      ├── [本地检测分支]
      │    ├── std::process::Command("ps aux | grep dmserver")  ──► running/stopped
      │    ├── TcpStream::connect("127.0.0.1:PORT")             ──► listening/closed
      │    └── std::process::Command("disql ...V$INSTANCE")     ──► PRIMARY/STANDBY/OPEN/unknown
      │
      ├── [远程节点并发分支] futures::join_all
      │    └── 每个 NodeConfig:
      │         ├── SshSession::connect(host, 22, ssh_creds) [timeout 5s]
      │         │    ├── 失败 ──► NodeStatus { error: Some(reason) }
      │         │    └── 成功:
      │         │         ├── exec("ps aux | grep dmserver | grep -v grep") ──► running/stopped
      │         │         ├── exec("ss -tlnp | grep ':{PORT}'")             ──► listening/closed
      │         │         └── exec("echo '...' | disql SYSDBA/SYSDBA@...")  ──► role string
      │         └── NodeStatus { process, port, role }
      │
      └── 表格格式化 ──► println! 对齐输出
```

### Recommended Project Structure

```
src/
├── status/
│   └── mod.rs          # 整个 status 功能（单文件即可，约 150-200 行）
├── cli.rs              # 新增 Status(StatusArgs) 变体
├── main.rs             # 新增 mod status + dispatch 分支
└── ...（其余不变）
```

### Pattern 1: 本地进程检测

**What:** 用 `std::process::Command` 运行 `ps aux | grep dmserver | grep -v grep`，输出非空即 running。

**When to use:** 本地节点检测（无 SSH 通道可用）。

**Example:**
```rust
// [ASSUMED] — 基于 D-06 决策，与现有 phases.rs 的 disql 调用结构对称
fn detect_local_process() -> &'static str {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg("ps aux | grep dmserver | grep -v grep")
        .output();
    match output {
        Ok(out) if !out.stdout.is_empty() => "running",
        _ => "stopped",
    }
}
```

### Pattern 2: 本地 TCP 端口检测（单次 connect）

**What:** 一次性 `TcpStream::connect`，连通即 listening，拒绝连接即 closed。与 `health.rs::wait_tcp_ready` 的区别是不轮询。

**When to use:** 本地端口状态快照（status 命令不需要等待就绪）。

**Example:**
```rust
// [ASSUMED] — 基于 D-07 决策，复用 tokio::net::TcpStream
async fn detect_local_port(port: u16) -> &'static str {
    use tokio::time::{timeout, Duration};
    let addr = format!("127.0.0.1:{}", port);
    match timeout(Duration::from_secs(1), tokio::net::TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => "listening",
        _ => "closed",
    }
}
```

### Pattern 3: 远程节点并发查询（`futures::join_all`）

**What:** 对 `ClusterSpecificConfig::nodes` 的每个 `NodeConfig` 并发 spawn 查询任务，收集 `NodeStatus` 结果。

**When to use:** STAT-02 远程多节点查询，避免串行等待。

**Example:**
```rust
// [ASSUMED] — 与 preflight.rs::preflight_all_nodes 结构对称
use futures::future::join_all;

let futures = cluster.nodes.iter().map(|node| {
    let node = node.clone();
    let port = cluster.dminit.port;
    async move { query_remote_node(&node, port).await }
});
let results: Vec<NodeStatus> = join_all(futures).await;
```

### Pattern 4: disql 输出解析

**What:** 从 `echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | disql ...` 输出中提取角色字符串。

**When to use:** STAT-03 角色列（PRIMARY/STANDBY/OPEN）。

已验证的输出格式（来自 `phases.rs` 测试数据）：
```
STATUS$   MODE$
OPEN      STANDBY
```

解析逻辑（Claude's Discretion 范围）：
```rust
// [ASSUMED] — 基于 phases.rs 中已验证的输出格式
fn parse_role_from_disql(output: &str) -> String {
    // 现有代码检测 OPEN+STANDBY 组合；status 命令需要区分更多状态
    if output.contains("PRIMARY") {
        "PRIMARY".to_string()
    } else if output.contains("OPEN") && output.contains("STANDBY") {
        "STANDBY".to_string()
    } else if output.contains("OPEN") {
        "OPEN".to_string()   // 独立运行的单机或主节点刚启动
    } else {
        "unknown".to_string()
    }
}
```

**重要细节：** 现有 `phases.rs:278` 的 disql 命令用 `shell_quote` 转义了 `install_path`，status 模块也需要引用 `crate::common::shell_quote`。

### Pattern 5: 对齐表格格式化（手动，无 crate）

**What:** 计算每列最大宽度，用 `format!("{:<width$}", value, width=col_width)` 左对齐输出。

**When to use:** D-16 锁定无额外 crate 依赖。

**Example:**
```rust
// [ASSUMED] — 基于 D-17 的示例输出格式
fn format_table(rows: &[TableRow]) -> String {
    // 动态列宽（Claude's Discretion 建议选项）
    let host_width = rows.iter().map(|r| r.host.len()).max().unwrap_or(4).max(4);
    let node_width = rows.iter().map(|r| r.node.len()).max().unwrap_or(4).max(4);

    let mut out = String::new();
    // 表头
    out.push_str(&format!(
        "{:<nw$}  {:<hw$}  {:<7}  {:<4}  {}\n",
        "Node", "Host", "Process", "Port", "Role",
        nw = node_width, hw = host_width,
    ));
    // 分隔线
    out.push_str(&format!(
        "{:-<nw$}  {:-<hw$}  {:-<7}  {:-<4}  {}\n",
        "", "", "", "", "-------",
        nw = node_width, hw = host_width,
    ));
    // 数据行
    for row in rows {
        out.push_str(&format!(
            "{:<nw$}  {:<hw$}  {:<7}  {:<4}  {}\n",
            row.node, row.host, row.process, row.port, row.role,
            nw = node_width, hw = host_width,
        ));
    }
    out
}
```

### Anti-Patterns to Avoid

- **在 `status::run` 中直接 `unwrap` SSH 错误：** D-15 要求单节点失败不影响其他节点，错误必须转化为 `NodeStatus { error: Some(msg) }` 而非传播。
- **串行查询远程节点：** 与 D-14 矛盾，且 UX 差（多节点时等待时间线性增长）。
- **不加超时直接 connect SSH：** SSH 连接到不可达节点会永久阻塞，必须套 `tokio::time::timeout`。
- **对 disql exit_code != 0 使用 `?` 传播：** disql 连接失败时 exit_code 非 0，status 命令应静默处理为 `unknown` 而非报错。
- **`ss` 命令 exit_code 1 当作错误：** 与 `preflight.rs:35` 一致，`grep` 无匹配时返回 1，需显式处理为"端口未监听"。

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SSH 连接 + 命令执行 | 自行封装 russh | `SshSession::connect` + `CommandRunner::exec` | 已有完整实现含 TOFU、密钥/密码双路认证 |
| 并发 futures | tokio::spawn + channel 收集 | `futures::join_all` | 已有，preflight.rs 验证过此模式 |
| 配置加载 | 重新解析 TOML | `config::load_config()` | 现有函数返回 `LoadedConfig` 枚举，包含完整节点列表 |
| 超时控制 | sleep + flag | `tokio::time::timeout` | tokio 内置，健壮 |
| Shell 参数转义 | 手动拼接 | `crate::common::shell_quote` | 现有函数，防命令注入 |

**Key insight:** Phase 6 是在现有 15+ 个源文件的基础上添加约 200 行代码，80% 是现有模式的组合，而非新发明。

## Common Pitfalls

### Pitfall 1: ss 命令不存在
**What goes wrong:** 远程节点（老系统如 CentOS 6 / 无 iproute2 的 minimal 镜像）上 `ss` 不存在，导致 exec 失败且输出为"命令未找到"，误报为端口关闭。
**Why it happens:** `ss` 是 iproute2 包提供的，minimal Docker 镜像或老发行版可能没有安装。
**How to avoid:** 对 `ss` 命令的 `SshError::ExecFailed` 做特殊处理——exit_code 127 视为"无法检测"（显示 `unknown`），而非 `closed`。或者改用 `netstat -tlnp | grep ':{PORT}'`（net-tools 包）作为备用命令。preflight.rs 也有同样问题但未处理，Phase 6 可以做得更好。
**Warning signs:** exec 返回 exit_code 127，stdout 含 "command not found"。

### Pitfall 2: disql 路径不固定
**What goes wrong:** 不同节点的 DM 安装路径不同（`/opt/dmdbms` vs `/home/dmdba/dmdbms`），status 命令硬编码路径会找不到 disql 二进制。
**Why it happens:** `DminitConfig::install_path` 是每集群配置，但 status 命令如果不读取 cluster config 就不知道安装路径。
**How to avoid:** 从 `ClusterSpecificConfig::dminit.install_path` 取路径（集群模式下），本地模式下从 `InstallConfig::install_path` 取。无 config 时（no-config 模式）尝试固定路径 `/opt/dmdbms/bin/disql` 和 `/home/dmdba/dmdbms/bin/disql`，都找不到则 Role 显示 `unknown`。
**Warning signs:** disql exec 返回 exit_code 127（命令未找到）。

### Pitfall 3: SSH 无超时导致挂起
**What goes wrong:** 远程节点防火墙规则导致 TCP SYN 包被丢弃（而非 RST 重置），SSH connect 永久阻塞，status 命令不返回。
**Why it happens:** `russh::client::connect` 无内置超时，默认等待 OS 的 TCP 连接超时（可能 75 秒）。
**How to avoid:** 用 `tokio::time::timeout(Duration::from_secs(5), SshSession::connect(...)).await`，超时后直接将该节点标记为 `ERROR: 连接超时`。
**Warning signs:** status 命令在某节点不可达时长时间无响应。

### Pitfall 4: `grep` 退出码污染错误处理
**What goes wrong:** `CommandRunner::exec` 在 exit_code != 0 时返回 `SshError::ExecFailed`，而 `grep` 在无匹配时返回 exit_code 1（这是正常行为），会被误处理为错误。
**Why it happens:** `session.rs:195-199` 的 `collect_exec_output` 遇 exit_code != 0 直接返回 Err。
**How to avoid:** 与 `preflight.rs:35` 相同的处理模式：对 `grep` 命令显式匹配 `SshError::ExecFailed { exit_code: 1, .. }` 作为"无匹配"的 Ok 分支，而非 Err。
**Warning signs:** 端口未监听时 `check_port` 报错而非返回 "closed"。

### Pitfall 5: NodeRole 转字符串显示
**What goes wrong:** D-18 要求 Node 列显示来自 `NodeRole` 枚举（Primary/Standby/Monitor），但枚举的 `Debug` 实现可能与期望格式不符。
**Why it happens:** `NodeRole` 用 `#[serde(rename_all = "lowercase")]` 反序列化，但 `Debug` 输出是 `Primary` 而非 `primary`。
**How to avoid:** 实现一个辅助函数 `node_role_label(role: NodeRole) -> &'static str`，返回 `"primary"` / `"standby"` / `"monitor"`（或按 D-18 的显示格式）；不依赖 `Debug` 或 `Display`。

## Code Examples

### disql 命令构造（验证自现有代码）
```rust
// Source: src/cluster/phases.rs:277-282 [VERIFIED: codebase grep]
// 现有代码，status 模块直接参考此模式
let cmd = format!(
    "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
    crate::common::shell_quote(&install_path),
    crate::common::shell_quote(&sysdba_password),
    port,
);
```

### ss 端口检测（验证自现有代码）
```rust
// Source: src/cluster/preflight.rs:25-41 [VERIFIED: codebase grep]
// grep 返回 exit_code 1 表示无匹配（端口空闲），需显式处理
let cmd = format!("ss -tlnp | grep ':{port}'");
match runner.exec(&cmd).await {
    Ok((stdout, _)) if !stdout.is_empty() => { /* listening */ }
    Ok(_) => { /* closed（grep 返回 0 但输出为空，这种情况理论上不出现） */ }
    Err(SshError::ExecFailed { exit_code: 1, .. }) => { /* 无匹配 = closed */ }
    Err(e) => { /* 真正的错误 */ }
}
```

### 并发节点查询（验证自现有代码）
```rust
// Source: src/cluster/preflight.rs:101-127 [VERIFIED: codebase grep]
// join_all 收集所有结果，不因单节点失败中止
use futures::future::join_all;
let results: Vec<NodeStatus> = join_all(
    nodes.iter().map(|node| query_remote_node(node, port))
).await;
```

### StatusArgs 结构（新增，D-01）
```rust
// [ASSUMED] — 基于 D-01 决策，与现有 ValidateArgs 风格对称
#[derive(clap::Args)]
pub struct StatusArgs {
    // 无必填参数；config.toml 自动从当前目录发现
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `netstat -tlnp` | `ss -tlnp` | iproute2 替代 net-tools（约 2014 年） | 大多数现代 Linux 发行版有 `ss`，但极简镜像可能没有 |
| `grep` exit_code 被视为错误 | 显式处理 exit_code 1 | preflight.rs 建立的模式 | 已验证，status 模块必须沿用 |

**Deprecated/outdated:**
- `netstat`: 许多系统不再默认安装 net-tools，但作为 `ss` 的回退方案仍有价值

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | 本地 `disql` 检测用 `std::process::Command`（非 SSH）；`install_path` 来自 config 或固定默认 | Code Examples | 若 disql 路径不匹配，Role 显示 unknown——可接受，非致命 |
| A2 | disql 输出包含 `STATUS$` / `MODE$` 列名及 `PRIMARY` / `STANDBY` / `OPEN` 关键词 | Pattern 4 | 若 DM 不同版本输出格式不同，解析失效；但 phases.rs 测试数据已验证此格式 |
| A3 | 动态列宽（按最长 host 计算）是 Claude's Discretion 范围的推荐实现 | Pattern 5 | 若选固定列宽，对长 host 名可能对齐错位——影响观感但不影响功能 |
| A4 | SSH 连接超时建议 5 秒，disql 查询超时建议 5 秒 | Common Pitfalls 3 | 若网络较慢，5s 可能不够——但用户无感知，可后续调整 |

## Open Questions (RESOLVED)

1. **no-config 本地模式下的 disql 路径** — **RESOLVED (2026-06-15)**
   - What we know: 无 config 时没有 `install_path`；D-08 说"若失败则显示 unknown"
   - What's unclear (历史问题): 是否尝试多个候选路径，还是直接显示 unknown？
   - **Resolution:** 采用单一固定路径 `/opt/dmdbms/bin/disql`（DminitConfig 默认值）。如果该路径下 disql 不存在或执行失败（exit_code 127 或非 0），Role 列直接显示 `"unknown"`，不尝试其他候选路径。理由：简单可靠；DBA 通常使用官方推荐路径；no-config 模式本身就是"开发者临时查询"场景，对边缘路径覆盖率要求低。
   - **Impact on plan:** Task 1 的 `query_local_role` 在 no-config 分支硬编码 `install_path = "/opt/dmdbms"`，无需多路径探测逻辑。

2. **`ss` 命令不可用的处理级别** — **RESOLVED (2026-06-15)**
   - What we know: ss 不在所有环境中存在；preflight.rs 未处理此边界
   - What's unclear (历史问题): 是否要做 `ss` + `netstat` 双路回退
   - **Resolution:** 不做 `netstat` 回退。当 `ss` 命令返回 exit_code 127（command not found）时，将 port 列设为 `"unknown"`、role 列因端口状态不确定也设为 `"unknown"`（不再尝试 disql 查询）。理由：(a) 实现简单（单一代码路径）；(b) 现代 Linux 发行版 ss 几乎都可用，回退收益低；(c) `netstat` 在不同发行版输出格式也有差异，再加一层解析风险更高；(d) 用户可见 "unknown" 比错误的 "closed" 更安全。
   - **Impact on plan:** Task 2 的 `check_remote_port` 对 `SshError::ExecFailed { exit_code: 127, .. }` 显式返回 `"unknown"`（已在 Task 2 action 与 test_check_remote_port_ss_missing_exit127 测试中体现），不引入 netstat 备用命令。

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust / cargo | 构建 | ✓ | 1.96.0 | — |
| `ps` | 本地进程检测 | ✓（macOS/Linux 标准） | — | — |
| `ss` (iproute2) | 远程端口检测 | 目标 Linux 通常有 | — | 无回退（按 Open Q2 决议，exit 127 → "unknown"） |
| `disql` | 角色查询 | 运行时（DM 安装后） | — | Role 显示 unknown |
| tokio::net::TcpStream | 本地端口检测 | ✓（tokio 已引入） | — | — |
| `SshSession` | 远程节点连接 | ✓（russh 已引入） | — | — |

**Missing dependencies with no fallback:** 无阻塞项。

**Missing dependencies with fallback:**
- DM 未安装时 Role 显示 `unknown`（per Open Q1 决议，no-config 模式无路径探测）
- `ss` 不存在时 port/role 均显示 `unknown`（per Open Q2 决议）

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test（内置） + tracing-test 0.2 |
| Config file | Cargo.toml dev-dependencies（已配置） |
| Quick run command | `cargo test -p dm-database-installer status` |
| Full suite command | `cargo test -p dm-database-installer` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| STAT-01 | 本地进程检测返回 running/stopped | unit | `cargo test status::tests::test_detect_local_process` | ✅ inline (Task 1) |
| STAT-01 | 本地端口检测返回 listening/closed | unit | `cargo test status::tests::test_check_local_port_closed` | ✅ inline (Task 1) |
| STAT-02 | 远程节点 SSH 失败时显示 ERROR 不中止 | unit (MockRunner) | `cargo test status::tests::test_query_remote_node_with_runner_error_isolation` | ✅ inline (Task 2) |
| STAT-02 | 多节点错误隔离（MockRunner）| unit | `cargo test status::tests::test_query_remote_node_with_runner_error_isolation` | ✅ inline (Task 2) |
| STAT-03 | disql 输出解析 PRIMARY/STANDBY/OPEN | unit | `cargo test status::tests::test_parse_role_from_disql_*` | ✅ inline (Task 1) |
| STAT-03 | 表格格式化输出对齐 | unit | `cargo test status::tests::test_format_table_*` | ✅ inline (Task 3) |

### Sampling Rate
- **Per task commit:** `cargo test -p dm-database-installer status`
- **Per wave merge:** `cargo test -p dm-database-installer`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [x] `src/status/mod.rs` — 包含所有 status 功能 + `#[cfg(test)] mod tests`，覆盖 STAT-01/02/03 — **inline in Task 1 via TDD**

*(现有测试基础设施（MockRunner、tracing-test）已完备，Wave 0 在 Task 1 中通过 TDD 内联完成：Task 1 先写 7 个失败测试再写实现)*

## Security Domain

`security_enforcement` 未显式禁用，故包含此章节。

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | 否 | SSH 认证沿用现有 SshSession，已在 Phase 3/4 建立 |
| V3 Session Management | 否 | — |
| V4 Access Control | 否 | — |
| V5 Input Validation | 是（低风险） | Port 号来自 config（已验证），`shell_quote` 转义 disql 路径参数 |
| V6 Cryptography | 否 | — |

### Known Threat Patterns for this Stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| disql 参数注入 | Tampering | `crate::common::shell_quote()` 转义 install_path 和 sysdba_password |
| SSH TOFU 中间人 | Spoofing | 现有 TofuHandler（生产建议 host_key_fingerprint，但 Phase 6 不修复） |
| ps aux 输出中含恶意进程名 | Spoofing | 只检测 dmserver 字符串存在，不执行输出，无注入风险 |

**关键点：** D-13 延用 SYSDBA/SYSDBA 硬编码（CR-02 已知问题），status 命令不引入新的安全降级。

## Sources

### Primary (HIGH confidence)
- `src/cluster/preflight.rs` [VERIFIED: codebase grep] — `ss -tlnp` 端口检测模式，`grep` exit_code 1 处理
- `src/cluster/phases.rs:277-282` [VERIFIED: codebase grep] — `disql SYSDBA/{}@localhost:{}` 命令格式，disql 输出解析（`OPEN`+`STANDBY` 检测）
- `src/cluster/health.rs` [VERIFIED: codebase grep] — TCP connect 单次检测思路（`TcpStream::connect`）
- `src/cluster/preflight.rs:101-127` [VERIFIED: codebase grep] — `futures::future::join_all` 并发节点查询模式
- `src/common/ssh/session.rs` [VERIFIED: codebase grep] — `SshSession::connect` + `exec` 完整签名
- `src/common/ssh/mock.rs` [VERIFIED: codebase grep] — `MockRunner` 测试基础设施，可直接用于 status 单元测试
- `src/config/cluster.rs:190-244` [VERIFIED: codebase grep] — `NodeConfig` 结构（role/host/ssh 字段），`ClusterSpecificConfig::nodes`

### Secondary (MEDIUM confidence)
- `06-CONTEXT.md` D-01~D-18 — 全部实现决策（用户锁定）

### Tertiary (LOW confidence)
- 无

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — 全部为现有 Cargo.toml 依赖，无新包
- Architecture: HIGH — 完全基于代码库现有模式
- Pitfalls: HIGH（Pitfall 2/3/4）/ MEDIUM（Pitfall 1/5）— 部分来自代码库直接验证，部分来自类似问题推断
- 测试架构: HIGH — MockRunner 和测试基础设施已完备

**Research date:** 2026-06-14
**Last updated:** 2026-06-15 (Open Questions resolved)
**Valid until:** 2026-07-14（依赖版本稳定，codebase 模式可能随 Phase 7 演化）
