---
phase: 05-rws
reviewed: 2026-06-14T00:00:00Z
depth: standard
files_reviewed: 4
files_reviewed_list:
  - src/cluster/checkpoint.rs
  - src/cluster/mod.rs
  - src/cluster/phases.rs
  - src/cluster/rws/mod.rs
findings:
  critical: 2
  warning: 3
  info: 2
  total: 7
status: issues_found
---

# Phase 05: Code Review Report

**Reviewed:** 2026-06-14
**Depth:** standard
**Files Reviewed:** 4
**Status:** issues_found

## Summary

审查范围覆盖集群检查点（`checkpoint.rs`）、集群分发入口（`mod.rs`）、阶段函数库（`phases.rs`）和读写分离部署（`rws/mod.rs`）。整体结构清晰，异步并行处理正确，错误传播一致。

发现 2 个 BLOCKER：其一是 `SshCredentials` 缺少 `ssh_port` 字段，导致非标准 SSH 端口集群节点无法连接；其二是硬编码的 SYSDBA 明文密码出现在写入远端节点的 shell 命令中。3 个 WARNING 涉及死参数、`println!` 混入结构化日志输出、以及检查点 TOCTOU 竞态。2 个 INFO 涉及顺序轮询低效和 `primary_standby` 与 `rws` 的对称性缺失。

---

## Critical Issues

### CR-01: 集群节点 SSH 端口硬编码为 22，不可配置

**File:** `src/cluster/rws/mod.rs:14`（同样存在于 `src/cluster/primary_standby/mod.rs:14`）

**Issue:** `ssh::SshSession::connect(&node.host, 22, &node.ssh)` 将 SSH 端口写死为 `22`。`NodeConfig.ssh` 的类型是 `SshCredentials`，该结构体没有 `ssh_port` 字段（`ssh_port` 字段仅存在于独立部署使用的 `SshTarget` 结构中）。任何通过非标准端口（如 `2222`、`60022`）暴露 SSH 的集群节点在运行时都会连接失败，且用户没有任何配置入口可以修改该端口。

**Fix:**

1. 在 `src/config/ssh.rs` 的 `SshCredentials` 中增加 `ssh_port` 字段（或将 `ssh_port` 移至 `NodeConfig` 顶层）：

```rust
// config/ssh.rs
#[derive(Debug, Deserialize, Clone)]
pub struct SshCredentials {
    pub user: String,
    pub identity_file: Option<PathBuf>,
    #[serde(skip_serializing, default)]
    pub password: Option<String>,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
}
fn default_ssh_port() -> u16 { 22 }
```

2. 在连接时使用该字段：

```rust
// rws/mod.rs (同理 primary_standby/mod.rs)
let session = ssh::SshSession::connect(&node.host, node.ssh.ssh_port, &node.ssh)
    .await
    .map_err(|e| anyhow::anyhow!("连接节点 {} 失败: {}", node.host, e))?;
```

---

### CR-02: SYSDBA 明文密码硬编码写入远端 shell 命令

**File:** `src/cluster/phases.rs:277-279`（同样存在于 `src/cluster/deploy.rs:301`）

**Issue:** `wait_for_standby_open_impl` 构造的命令为：

```
echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | '/opt/dmdbms'/bin/disql SYSDBA/SYSDBA@localhost:5236
```

`SYSDBA/SYSDBA` 是明文默认密码，以字面量形式写入命令字符串，并通过 SSH 发送到远端节点。存在以下风险：

1. **命令行明文泄露：** 在目标节点上执行时，进程列表（`ps aux`）可能短暂暴露含密码的完整命令行。
2. **无法覆盖密码：** DBA 若在安装时修改了 SYSDBA 密码（达梦默认允许且推荐），此硬编码逻辑将在初始化后立即失效，导致 `disql` 认证失败。
3. **`deploy.rs:301` 的 `configure_database_role` 函数同样使用了 `SYSDBA/SYSDBA`**，影响整个集群角色配置流程。

**Fix:** 在 `ClusterSpecificConfig`（或 `DminitConfig`）中增加可选的 `sysdba_password` 字段，并通过环境变量或 stdin 方式传入密码，避免出现在命令行：

```rust
// phases.rs - 使用 disql 的 stdin 注入方式，避免密码出现在命令行
let cmd = format!(
    "printf 'SYSDBA\\n{}\\n' | {}/bin/disql -S <<'EOF'\nSELECT STATUS$,MODE$ FROM V$INSTANCE;\nEOF",
    shell_quote(&sysdba_password),
    shell_quote(&dminit.install_path),
);
```

短期最低限度修复：至少将密码从配置读取而非硬编码：

```rust
// ClusterSpecificConfig 增加字段
#[serde(default = "default_sysdba_password")]
pub sysdba_password: String,

fn default_sysdba_password() -> String { "SYSDBA".to_string() }
```

---

## Warnings

### WR-01: `run_read_routing_phase` 的 `specific` 参数是死参数

**File:** `src/cluster/phases.rs:315-320`

**Issue:** 函数签名接受 `specific: &ClusterSpecificConfig`，但函数体第一行即 `let _ = specific;`，该参数完全未被使用。这会误导调用者和维护者，并在函数签名层面暴露出设计意图与实现之间的差距。

```rust
pub async fn run_read_routing_phase(
    specific: &ClusterSpecificConfig,  // 未使用
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    let _ = specific;  // 显式丢弃
```

**Fix:** 移除该参数（并更新 `rws/mod.rs:51` 的调用处）；若预期未来会使用，改用 `#[allow(unused_variables)]` 并添加注释说明原因：

```rust
// 如果确认此参数将来需要（如路由策略配置），保留并注释：
// specific: &ClusterSpecificConfig,  // reserved for future routing policy config

// 如果当前不需要，直接去掉：
pub async fn run_read_routing_phase(
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()> {
```

---

### WR-02: `checkpoint.rs::load_from` 混用 `println!` 与结构化日志

**File:** `src/cluster/checkpoint.rs:55`

**Issue:** 检测到检查点后使用 `println!` 输出用户提示，而同文件的其他所有消息均通过 `tracing::debug!` / `tracing::warn!` 输出。项目使用 `tracing` + `tracing-subscriber` 作为统一日志框架（CLAUDE.md 技术栈要求），`println!` 会绕过日志过滤、格式化和 non-TTY 检测（CI 环境下会产生裸文本混入 JSON 输出）。

```rust
// 当前（错误）
println!("[续] 检测到检查点，从上次进度继续安装");

// 应改为
tracing::info!("[续] 检测到检查点，从上次进度继续安装");
```

**Fix:** 将 `println!` 替换为 `tracing::info!`，保持与同文件和整个项目日志风格一致。

---

### WR-03: `checkpoint.rs::load_from` 存在 TOCTOU 竞态

**File:** `src/cluster/checkpoint.rs:44-47`

**Issue:** 先通过 `path.exists()` 检查文件是否存在，再用 `std::fs::read_to_string(&path)?` 读取内容。在两次调用之间，文件可能被其他进程或操作系统删除（例如用户手动清理 `/tmp`，或同一主机上的并发部署实例）。此时 `read_to_string` 返回 `Err`，该错误向上传播为安装失败，而正确行为应等同于文件不存在（`Ok(None)`）。

```rust
// 当前（有竞态）
if !path.exists() {
    return Ok(None);
}
let content = std::fs::read_to_string(&path)?;  // 可能因竞态失败

// 修复：用 match 处理 NotFound
let content = match std::fs::read_to_string(&path) {
    Ok(c) => c,
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(e) => return Err(e.into()),
};
```

---

## Info

### IN-01: `rws::run_with_runners` 与 `primary_standby::run_with_runners` 设计不对称

**File:** `src/cluster/rws/mod.rs:30-55` vs `src/cluster/primary_standby/mod.rs:30-52`

**Issue:** `rws::run_with_runners` 有检查点逻辑（通过 `run_early_checkpoints` / `run_init_restore_checkpoints`），而 `primary_standby::run_with_runners` 完全没有。两者部署的是相同的 11 个阶段（rws 多一个 `run_read_routing_phase`），但只有 rws 支持断点续传。如果 `primary_standby` 部署在备份还原阶段失败，用户需要从头重新执行全部操作（包括重新下载安装包、dminit 等耗时步骤）。

**Fix:** 将检查点逻辑提取到 `phases` 模块中的共享函数，或为 `primary_standby::run_with_runners` 也增加相同的检查点门控。

---

### IN-02: 只读备库轮询为顺序执行，不支持并发

**File:** `src/cluster/phases.rs:330-332`

**Issue:** `run_read_routing_phase` 对多个只读备节点的健康检查是顺序的：

```rust
for (node, runner) in &readonly_standbys {
    wait_for_standby_open(node, dminit, runner.as_ref()).await?;
}
```

每个节点的最长等待时间为 `MAX_RETRIES(24) * POLL_INTERVAL_SECS(5) = 120s`。在有 N 个只读备节点的场景下，等待时间线性叠加为 N * 120s。实际上各节点状态是独立的，可以并发轮询。

**Fix:** 使用 `futures::future::try_join_all`：

```rust
let futs: Vec<_> = readonly_standbys
    .iter()
    .map(|(node, runner)| wait_for_standby_open(node, dminit, runner.as_ref()))
    .collect();
futures::future::try_join_all(futs).await?;
```

---

_Reviewed: 2026-06-14_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
