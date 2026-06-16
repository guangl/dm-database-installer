---
phase: 05-rws
reviewed: 2026-06-14T10:30:00Z
depth: standard
files_reviewed: 4
files_reviewed_list:
  - src/cluster/checkpoint.rs
  - src/cluster/mod.rs
  - src/cluster/phases.rs
  - src/cluster/rws/mod.rs
findings:
  critical: 3
  warning: 4
  info: 3
  total: 10
status: issues_found
---

# Phase 05: Code Review Report

**Reviewed:** 2026-06-14T10:30:00Z
**Depth:** standard
**Files Reviewed:** 4
**Status:** issues_found

## Summary

审查覆盖 RWS（读写分离）集群部署的四个核心模块：检查点持久化（`checkpoint.rs`）、集群分发入口（`mod.rs`）、通用阶段函数库（`phases.rs`）和 RWS 专用流程（`rws/mod.rs`）。代码结构分层合理，异步并行模式一致，检查点机制设计清晰。

本次审查同时参照被调用模块（`deploy.rs`）做跨文件验证，发现以下核心问题：

1. **RWS 的核心功能缺失**：`configure_read_only_standby()` 在整个代码库中只有定义，从未被调用。`run_read_routing_phase` 只轮询等待 OPEN 状态，但整个部署流程中没有任何步骤执行 `alter database open read only`，导致只读备库永远停在 MOUNT 状态，轮询超时后部署失败。
2. **硬编码 SSH 端口**：集群节点 SSH 连接端口硬编码为 22，`NodeConfig` 中无 SSH 端口字段，非标准端口场景连接必然失败。
3. **SYSDBA 明文密码硬编码写入 shell 命令**：多处 `disql SYSDBA/SYSDBA@localhost:PORT` 命令字面量硬编码，DBA 修改默认密码后整个集群部署流程失效。

## Critical Issues

### CR-01: 只读备库从未被打开——RWS 核心功能永远失效

**File:** `src/cluster/phases.rs:314-335` / `src/cluster/deploy.rs:439`

**Issue:**
`run_read_routing_phase` 的设计意图是等待只读备库进入 `STATUS$=OPEN MODE$=STANDBY`，但整个 RWS 部署流程中**从不调用 `deploy::configure_read_only_standby`**。

DM 主备集群中，备库以 `mount` 模式启动后处于 `STATUS$=MOUNT`，必须显式执行 `alter database open read only;` 才能进入 OPEN 状态。当前 `run_startup_phase` 对备节点只执行了 `alter database standby;`（配置角色），没有执行 `open read only`。

结果：`wait_for_standby_open_impl` 轮询 24 次（2 分钟），每次查询到的状态始终是 `MOUNT STANDBY` 而非 `OPEN STANDBY`，最终以超时错误终止部署。

全库搜索确认 `configure_read_only_standby` 只有定义，无任何调用点：
```
grep -rn configure_read_only_standby src/
# 输出仅有 src/cluster/deploy.rs:439（定义行）
```

**Fix:**
在 `run_read_routing_phase` 的轮询等待**之前**，对每个 `read_only=true` 的备节点执行 `configure_read_only_standby`：

```rust
// src/cluster/phases.rs run_read_routing_phase 修改
pub async fn run_read_routing_phase(
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][12/12] 开启只读备库并等待进入 OPEN 状态");
    let readonly_standbys: Vec<_> = runners
        .iter()
        .filter(|(node, _)| node.role == NodeRole::Standby && node.read_only)
        .collect();
    if readonly_standbys.is_empty() {
        tracing::warn!("[cluster][12/12] 无 read_only=true 的备节点，跳过只读验证");
        return Ok(());
    }
    // 先执行 alter database open read only
    for (node, runner) in &readonly_standbys {
        deploy::configure_read_only_standby(node, dminit, runner.as_ref()).await?;
    }
    // 再轮询等待 OPEN 状态
    for (node, runner) in &readonly_standbys {
        wait_for_standby_open(node, dminit, runner.as_ref()).await?;
    }
    tracing::info!("[cluster][12/12] 所有只读备库就绪");
    Ok(())
}
```

---

### CR-02: 集群节点 SSH 端口硬编码为 22，无法配置

**File:** `src/cluster/rws/mod.rs:14`（同样存在于 `src/cluster/primary_standby/mod.rs:14`）

**Issue:**
`ssh::SshSession::connect(&node.host, 22, &node.ssh)` 将 SSH 端口硬编码为 `22`。`NodeConfig` 的 `ssh` 字段类型为 `SshCredentials`，该结构体没有 `port` 字段。

对于生产环境中常见的非标准 SSH 端口配置（如 `2222`、`60022`、防火墙限制端口），用户没有任何配置入口可以修改连接端口，集群连接必然失败，且错误信息只显示"连接失败"而不提示端口问题。

**Fix:**
在 `SshCredentials`（或 `NodeConfig`）中增加可选的 `port` 字段：

```rust
// src/config/ssh.rs
#[derive(Debug, Deserialize, Clone)]
pub struct SshCredentials {
    pub user: String,
    pub identity_file: Option<PathBuf>,
    pub password: Option<String>,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
}
fn default_ssh_port() -> u16 { 22 }
```

连接时使用该字段：

```rust
// src/cluster/rws/mod.rs
let session = ssh::SshSession::connect(&node.host, node.ssh.port, &node.ssh)
    .await
    .map_err(|e| anyhow::anyhow!("连接节点 {}:{} 失败: {}", node.host, node.ssh.port, e))?;
```

---

### CR-03: SYSDBA 明文密码硬编码写入 shell 命令——密码修改后集群部署失效

**File:** `src/cluster/phases.rs:277-279` / `src/cluster/deploy.rs:202,301,402,446,481`

**Issue:**
以下所有场景均将 `SYSDBA/SYSDBA` 字面量硬编码写入 SSH 执行命令：

- `wait_for_standby_open_impl`（phases.rs:277）：`disql SYSDBA/SYSDBA@localhost:{port}`
- `stop_dmserver`（deploy.rs:202）：`disql SYSDBA/SYSDBA@localhost:{port}`
- `configure_database_role`（deploy.rs:301）：`disql SYSDBA/SYSDBA@localhost:{port}`
- `verify_node_role`（deploy.rs:402）：`disql SYSDBA/SYSDBA@localhost:{port}`
- `configure_read_only_standby`（deploy.rs:446）：`disql SYSDBA/SYSDBA@localhost:{port}`
- `configure_sqllog`（deploy.rs:481）：`disql SYSDBA/SYSDBA@localhost:{port}`

存在两个问题：

1. **功能性缺陷**：DBA 在 `dminit` 初始化时可设置自定义密码（DM 安装 XML 支持 `SYSDBA_PWD` 参数）。安装后若密码已被修改，所有后续 `disql` 调用将认证失败，导致集群初始化阶段（角色配置、验证、停库等）全部返回错误。当前安装流程没有任何配置项可以覆盖此密码。

2. **安全风险**：密码以明文形式出现在 SSH 命令字符串中，在目标节点的进程列表（`ps aux`）中短暂可见。

**Fix:**
在 `DminitConfig` 中增加 `sysdba_password` 字段，通过环境变量或配置读取，并在所有 `disql` 调用中使用该值：

```rust
// src/config/cluster.rs DminitConfig 增加
#[serde(default = "default_sysdba_password")]
pub sysdba_password: String,

fn default_sysdba_password() -> String { "SYSDBA".to_string() }
```

disql 调用中引用此字段而非硬编码：

```rust
let cmd = format!(
    "echo '{}' | {}/bin/disql SYSDBA/{}@localhost:{}",
    sql,
    shell_quote(&dminit.install_path),
    shell_quote(&dminit.sysdba_password),
    dminit.port,
);
```

## Warnings

### WR-01: `run_sqllog_phase` 对只读备库执行写操作，必然失败

**File:** `src/cluster/phases.rs:224-246` / `src/cluster/rws/mod.rs:49`

**Issue:**
`run_sqllog_phase` 对 `runners` 中**所有节点**并行执行 `configure_sqllog`，其中包含对处于只读 OPEN 状态的备库执行 `SP_SET_PARA_VALUE`（写操作）。只读备库无法接受 DDL/DML 写入，`disql` 会报错，`configure_sqllog` 内部重试 6 次（每次 5 秒，共 30 秒）后返回 `Err`，导致部署失败。

即使按照 CR-01 的修复让只读备库正确进入 OPEN 状态，此问题仍然存在。

**Fix:**
在 `run_sqllog_phase` 中过滤掉只读节点：

```rust
let futs: Vec<_> = runners
    .iter()
    .filter(|(node, _)| !node.read_only)   // 只读备库跳过 SQL 日志配置
    .map(|(node, runner)| {
        // ...
    })
    .collect();
```

---

### WR-02: `checkpoint.rs::load_from` 存在 TOCTOU 竞态

**File:** `src/cluster/checkpoint.rs:44-47`

**Issue:**
先通过 `path.exists()` 检查文件是否存在，再用 `std::fs::read_to_string(&path)?` 读取内容。在两次调用之间，文件可能被删除（用户手动清理、操作系统 `/tmp` 清理、并发部署实例）。此时 `read_to_string` 返回 `Err(NotFound)`，向上传播为安装失败，而正确行为应等同于文件不存在（`Ok(None)`）。

```rust
// 当前（有竞态）
if !path.exists() {
    return Ok(None);
}
let content = std::fs::read_to_string(&path)?;  // 若文件在此刻被删除，返回 Err
```

**Fix:**

```rust
let content = match std::fs::read_to_string(&path) {
    Ok(c) => c,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(e) => return Err(e.into()),
};
```

---

### WR-03: `run_verify_phase` 在只读备库打开前执行，验证结论不可信

**File:** `src/cluster/rws/mod.rs:50-51` / `src/cluster/phases.rs:248-263`

**Issue:**
`run_verify_phase`（行 50）在 `run_read_routing_phase`（行 51）之前执行。在 CR-01 修复前，只读备库始终处于 `STATUS$=MOUNT`；即使 CR-01 修复后，`run_verify_phase` 对 Standby 节点只验证 `MODE$=STANDBY`（不验证 `STATUS$`），因此它在 MOUNT 和 OPEN 状态下都会通过，给用户造成"验证通过"的误导。

对于 RWS 场景，只读备库的 OPEN 状态是关键指标，当前验证逻辑对此无感知。

**Fix:**
在 `verify_node_role` 中对 `read_only=true` 的备节点增加 STATUS$ 检查：

```rust
if expected_role == NodeRole::Standby && node.read_only {
    anyhow::ensure!(
        output.contains("OPEN"),
        "只读备节点 {} STATUS$ 验证失败：期望 STATUS$=OPEN，实际:\n{}",
        node.host, output
    );
}
```

并将 `run_verify_phase` 移到 `run_read_routing_phase` 之后执行。

---

### WR-04: `wait_for_standby_open_impl` 最后一次轮询不打印警告日志

**File:** `src/cluster/phases.rs:291-296`

**Issue:**
轮询警告仅在 `attempt < max_retries` 时输出。最后一次尝试（`attempt == max_retries`）失败后直接进入 `bail!`，操作员在日志中会看到警告突然停止然后出现错误，缺少"第 24/24 次仍失败"的记录，难以判断是否经历了完整的等待周期。

```rust
// 当前
if attempt < max_retries {
    tracing::warn!("...");
    tokio::time::sleep(...).await;
}
// 第 max_retries 次：无警告，直接 bail!
```

**Fix:**

```rust
tracing::warn!(
    "[node:{:?}] 备库尚未 OPEN（{}/{}），{}s 后重试",
    node.role, attempt, max_retries, interval_secs
);
if attempt < max_retries {
    tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
}
```

## Info

### IN-01: `checkpoint.rs::load_from` 混用 `println!` 与 tracing

**File:** `src/cluster/checkpoint.rs:55`

**Issue:**
检测到检查点后使用 `println!` 输出用户提示，而同文件的其他所有日志均通过 `tracing` 框架输出。项目使用 `tracing` + `tracing-subscriber` 作为统一日志框架（CLAUDE.md 技术栈规范），`println!` 绕过日志过滤和格式化，在 non-TTY 环境（CI、管道）中产生裸文本混入结构化日志流。

**Fix:**
```rust
// 替换
tracing::info!("[续] 检测到检查点，从上次进度继续安装");
```

---

### IN-02: `run_read_routing_phase` 的 `specific` 参数显式丢弃

**File:** `src/cluster/phases.rs:315,320`

**Issue:**
函数签名保留 `specific: &ClusterSpecificConfig` 但函数体第一行是 `let _ = specific;`。结合 CR-01 的修复，`specific` 在修复后仍不会被用到（只需要 `runners` 和 `dminit`）。死参数增加调用方的认知负担，且测试 `make_specific()` 需要构造一个空 TOML 对象传入。

**Fix:**
移除该参数并更新所有调用方（`rws/mod.rs:51` 和测试）：

```rust
pub async fn run_read_routing_phase(
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()> {
```

---

### IN-03: `default_oguid` 将今日日期硬编码为编译期常量

**File:** `src/config/cluster.rs:242`

**Issue:**
`fn default_oguid() -> u32 { 20260614 }` 将"今日"日期写死。文档注释说"默认今日日期 YYYYMMDD"，但实际上从明天起该值就永远是过去的日期。测试 `test_default_oguid_is_today`（行 753）验证的是固定值 `20260614`，未来任何时间运行测试都通过，但语义已错。

若同一套集群反复部署（如测试环境多次重建），不同批次会共享同一个 oguid，而 DM 要求 oguid 在守护系统内唯一。

**Fix:**
要么在运行时动态计算（引入 `chrono`），要么移除默认值强制用户显式配置 `oguid`，并在文档中说明其唯一性要求：

```toml
# rws.toml 必须显式指定，不提供默认值
oguid = 453331  # 必填，守护系统全局唯一标识
```

---

_Reviewed: 2026-06-14T10:30:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
