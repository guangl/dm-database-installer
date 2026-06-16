# Phase 5: RWS 读写分离集群 - Research

**Researched:** 2026-06-14
**Domain:** Rust 集群 checkpoint + async poll 模式（达梦 V$INSTANCE 轮询）
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**断点恢复（Checkpoint）**
- D-01: 文件名 `dm_cluster_checkpoint.json`，存放在当前工作目录（和 rws.toml 同级）
- D-02: 颗粒度：`preflight_done` / `install_done` / `primary_init_done` / `backup_done` / `standby_restore_done`
- D-03: standby_restore_done 之后的 phases 不单独打点；如失败，从 standby_restore 重试
- D-04: 部署全部完成后自动删除 checkpoint 文件
- D-05: 数据结构类似 `src/standalone/checkpoint.rs`，用 JSON，字段为各 phase 布尔标志

**只读备库开启时机**
- D-06: `alter database open read only` 不需要安装器执行——dmwatcher 自动完成状态转换
- D-07: `run_read_routing_phase` 的职责是等待 + 验证，不执行 SQL
- D-08: 备节点在 V$INSTANCE 的预期最终状态：`MODE$=STANDBY, STATUS$=OPEN`
- D-09: 等待参数：超时 120 秒，间隔 5 秒轮询一次（最多 24 次重试）

**实现位置**
- D-10: 在 `src/cluster/phases.rs` 新增 `run_read_routing_phase` 函数
- D-11: 函数签名：`pub async fn run_read_routing_phase(specific: &ClusterSpecificConfig, runners: &Runners, dminit: &DminitConfig) -> Result<()>`
- D-12: 逻辑：找 `role == Standby && read_only == true` 的节点，poll `SELECT STATUS$,MODE$ FROM V$INSTANCE` 直到 `STATUS$=OPEN`，或超时返回 Error
- D-13: 在 `src/cluster/rws/mod.rs` 中，在 `run_verify_phase` 之后调用（替换 TODO:50）

### Claude's Discretion

- checkpoint 文件的具体 JSON schema（字段命名、是否含时间戳）——与 standalone 保持一致即可
- 轮询 V$INSTANCE 时使用 `deploy::verify_node_role` 的变体还是新建专用的 `wait_for_standby_open` 函数

### Deferred Ideas (OUT OF SCOPE)

- `configure_read_only_standby()`（`deploy.rs:438`）目前无调用场景
- 备份传输后各 steps 的细粒度 checkpoint
- DMProxy 安装与配置

</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| RWS-01 | 用户可执行 `dm-installer install rws` 完成读写分离集群完整部署（主库 + 只读备库，端到端） | checkpoint 系统支持中断重跑；run_read_routing_phase 补全最后一步 |
| RWS-02 | 部署完成后备节点自动通过 SQL 配置为只读模式（READ_ONLY 标志） | D-06 确认 dmwatcher 自动完成；run_read_routing_phase 验证 STATUS$=OPEN |

</phase_requirements>

---

## Summary

Phase 5 的核心工作量分两块：

**第一块：集群 checkpoint 系统。** 当前 `rws/mod.rs::run_with_runners` 在各 phase 调用之间没有任何持久化，一旦中途失败（网络抖动、节点超时）必须从头重跑。对标 `src/standalone/checkpoint.rs`，新增 `src/cluster/checkpoint.rs`，以 JSON 文件记录 5 个高代价 phase（preflight/install/primary_init/backup/standby_restore）的完成状态，完成后自动删除。standalone 参考实现已有完整的 save/load/remove/roundtrip 模式，直接复用。

**第二块：`run_read_routing_phase`。** 该函数填补 `rws/mod.rs:50` 的 TODO。职责只有一件事：等待并验证 `read_only=true` 的备节点达到 `MODE$=STANDBY, STATUS$=OPEN`（dmwatcher 自动完成状态转换，安装器不执行额外 SQL）。轮询逻辑可复用 `deploy::verify_node_role` 的 disql 调用模式，加 `tokio::time::sleep` 重试循环。

**主要约束：** 函数长度限制 40 行（CLAUDE.md），需将 poll 循环提取为专用的 `wait_for_standby_open` 函数。所有代码已有完整的 MockRunner 基础设施，单元测试直接可测。

**Primary recommendation:** 新建 `src/cluster/checkpoint.rs`（模仿 standalone），然后在 `phases.rs` 末尾追加 `run_read_routing_phase`（拆分为 `wait_for_standby_open` 辅助函数），最后在 `rws/mod.rs` 嵌入 checkpoint gate 并替换 TODO。

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| 集群 checkpoint 持久化 | 控制机本地文件系统 | — | checkpoint 文件和 rws.toml 同级，记录控制流状态 |
| V$INSTANCE 状态查询 | 备节点远端（SSH） | — | disql 必须在备节点本地执行，通过 runner.exec 远程调用 |
| 只读状态转换 | dmwatcher 自动 | — | D-06 确认：安装器不负责，dmwatcher 在 Primary 注册完成后自动将备库从 MOUNT 转 OPEN |
| 轮询 + 超时控制 | 控制机 tokio 异步 | — | tokio::time::sleep 在控制机侧等待，不在备节点执行任何操作 |
| 函数调用顺序 | rws/mod.rs | phases.rs | mod.rs 编排调用顺序，phases.rs 持有具体实现 |

---

## Standard Stack

本 phase 不引入新依赖，完全复用已有 crate。

### Core（已在 Cargo.toml）

| Library | Purpose | How Used |
|---------|---------|----------|
| `tokio` | 异步运行时 | `tokio::time::sleep(Duration::from_secs(5))` 实现轮询间隔 |
| `anyhow` | 错误处理 | `anyhow::bail!` 超时错误；`anyhow::anyhow!` 错误包装 |
| `serde` + `serde_json` | checkpoint JSON 序列化 | 与 standalone checkpoint 完全相同的 derive 模式 |
| `tracing` | 结构化日志 | `[cluster][12/12]` 标记，poll 进度 warn/info |

### 无需新增依赖

slopcheck 扫描不适用（本 phase 不安装新 package）。

---

## Package Legitimacy Audit

本 phase 不安装任何新外部 package，全部复用 Cargo.toml 中已存在的依赖。

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

---

## Architecture Patterns

### System Architecture Diagram

```
rws/mod.rs::run_with_runners
    │
    ├─[preflight]─→ phases::run_preflight ──→ checkpoint.save(preflight_done)
    ├─[install]──→ phases::run_install_phase → checkpoint.save(install_done)
    ├─[init]─────→ phases::run_primary_init → checkpoint.save(primary_init_done)
    ├─[backup]───→ phases::run_backup_phase → checkpoint.save(backup_done)
    ├─[restore]──→ phases::run_standby_restore → checkpoint.save(standby_restore_done)
    │
    │  [以下失败时，重跑从 standby_restore_done 起，不单独打点]
    ├─[distribute]→ phases::run_distribute_phase
    ├─[startup]──→ phases::run_startup_phase
    ├─[watcher]──→ phases::run_watcher_phase
    ├─[monitor]──→ phases::run_monitor_phase
    ├─[sqllog]───→ phases::run_sqllog_phase
    ├─[verify]───→ phases::run_verify_phase
    └─[read_routing]→ phases::run_read_routing_phase ← 本 phase 新增
                         │
                         └─ wait_for_standby_open(node, runner, dminit)
                                │
                                ├─ loop (最多 24 次)
                                │    └─ disql: SELECT STATUS$,MODE$ FROM V$INSTANCE
                                │         ├─ STATUS$=OPEN ──→ Ok(())
                                │         └─ 否则 sleep 5s
                                └─ 超时 → Err("备节点 X 未在 120s 内达到 OPEN")
                         │
                         └─ checkpoint.remove() ← 全部成功后删除
```

### Recommended Project Structure

```
src/
├── cluster/
│   ├── checkpoint.rs        ← 新建（集群 checkpoint，本 phase 核心）
│   ├── phases.rs            ← 追加 run_read_routing_phase + wait_for_standby_open
│   ├── rws/
│   │   └── mod.rs           ← 嵌入 checkpoint gate，替换 TODO:50
│   └── deploy.rs            ← 不修改（verify_node_role 作为参考，不调用）
└── standalone/
    └── checkpoint.rs        ← 参考实现（不修改）
```

### Pattern 1: 集群 Checkpoint 结构体

**What:** JSON 文件记录各 phase 布尔完成标志，save/load/remove 三方法，完成后自动删除。

**When to use:** 每个高代价 phase 完成后立即调用 `cp.save()`；全部完成后调用 `ClusterCheckpoint::remove()`。

**示例（模仿 standalone/checkpoint.rs）：**
```rust
// src/cluster/checkpoint.rs
// [ASSUMED] — 根据 D-01~D-05 和 standalone 参考实现推导，未在外部文档验证

const FILE_NAME: &str = "dm_cluster_checkpoint.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClusterCheckpoint {
    #[serde(default)]
    pub preflight_done: bool,
    #[serde(default)]
    pub install_done: bool,
    #[serde(default)]
    pub primary_init_done: bool,
    #[serde(default)]
    pub backup_done: bool,
    #[serde(default)]
    pub standby_restore_done: bool,
}

impl ClusterCheckpoint {
    pub fn save(&self) -> Result<()> { /* serde_json::to_string_pretty + fs::write */ }
    pub fn load() -> Result<Option<Self>> { /* fs::read_to_string + serde_json::from_str */ }
    pub fn remove() -> Result<()> { /* fs::remove_file if exists */ }

    // 测试友好的 _to/_from 变体（同 standalone 模式）
    pub(crate) fn save_to(&self, dir: &Path) -> Result<()> { ... }
    pub(crate) fn load_from(dir: &Path) -> Result<Option<Self>> { ... }
    pub(crate) fn remove_from(dir: &Path) -> Result<()> { ... }
}
```

**关键差异 vs standalone：**
- standalone Checkpoint 含 `install_path` 作为匹配键（防止目录复用导致误读）
- ClusterCheckpoint 不需要 install_path 匹配键——`dm_cluster_checkpoint.json` 和 `rws.toml` 同目录，用户不会混淆
- standalone 含密码字段；ClusterCheckpoint 只有布尔标志

### Pattern 2: wait_for_standby_open 轮询函数

**What:** 对单个 read_only 备节点执行最多 24 次 disql 轮询，间隔 5s，等待 `STATUS$=OPEN`。

**When to use:** 在 run_read_routing_phase 内，对每个 `role==Standby && read_only==true` 的节点串行调用（通常只有 1 个节点）。

**示例（基于 deploy::verify_node_role 的 SQL 模式）：**
```rust
// src/cluster/phases.rs — 新增辅助函数
// [ASSUMED] — 根据 D-07~D-09 和 deploy.rs:395-436 推导

async fn wait_for_standby_open(
    node: &NodeConfig,
    dminit: &DminitConfig,
    runner: &dyn ssh::CommandRunner,
) -> Result<()> {
    let cmd = format!(
        "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/SYSDBA@localhost:{}",
        crate::common::shell_quote(&dminit.install_path),
        dminit.port,
    );
    for attempt in 1..=24u32 {
        let (stdout, exit_code) = runner.exec(&cmd).await
            .map_err(|e| anyhow::anyhow!("poll V$INSTANCE 失败: {}", e))?;
        if exit_code == 0 {
            let output = String::from_utf8_lossy(&stdout);
            if output.contains("OPEN") && output.contains("STANDBY") {
                tracing::info!("[node:{:?}] 只读备库已就绪 STATUS$=OPEN MODE$=STANDBY", node.role);
                return Ok(());
            }
        }
        if attempt < 24 {
            tracing::warn!("[node:{:?}] 备库尚未 OPEN（{}/24），5s 后重试", node.role, attempt);
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }
    anyhow::bail!("备节点 {} 未在 120s 内达到 STATUS$=OPEN MODE$=STANDBY", node.host)
}
```

**函数长度检查：** 以上约 22 行，满足 40 行上限。

### Pattern 3: run_read_routing_phase 主函数

```rust
// src/cluster/phases.rs — 追加
// [ASSUMED]

pub async fn run_read_routing_phase(
    specific: &ClusterSpecificConfig,
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][12/12] 等待只读备库进入 OPEN 状态");
    let readonly_standbys: Vec<_> = runners
        .iter()
        .filter(|(n, _)| n.role == NodeRole::Standby && n.read_only)
        .collect();
    if readonly_standbys.is_empty() {
        tracing::warn!("[cluster][12/12] 无 read_only=true 的备节点，跳过只读验证");
        return Ok(());
    }
    for (node, runner) in &readonly_standbys {
        wait_for_standby_open(node, dminit, runner.as_ref()).await?;
    }
    tracing::info!("[cluster][12/12] 所有只读备库就绪");
    Ok(())
}
```

**注意：** `specific` 参数在当前逻辑中未使用（只读节点信息从 runners 读取），但签名按 D-11 锁定必须包含，保持与其他 phase 函数的一致性，且未来可能需要。

### Pattern 4: rws/mod.rs checkpoint gate 嵌入

```rust
// src/cluster/rws/mod.rs::run_with_runners — 修改后的结构
// [ASSUMED]

pub async fn run_with_runners(...) -> Result<()> {
    let dminit = specific.dminit.clone();
    let mut cp = crate::cluster::checkpoint::ClusterCheckpoint::load()?
        .unwrap_or_default();

    if !cp.preflight_done {
        phases::run_preflight(&runners, &dminit).await?;
        cp.preflight_done = true; cp.save()?;
    } else {
        tracing::info!("[续] 跳过预检查（checkpoint）");
    }
    // ... 类似模式 for install / primary_init / backup / standby_restore

    // standby_restore_done 之后不打点，直接执行
    phases::run_distribute_phase(&specific, &runners, &dminit).await?;
    phases::run_startup_phase(...).await?;
    phases::run_watcher_phase(&runners, &dminit).await?;
    phases::run_monitor_phase(&specific, &runners, &dminit).await?;
    phases::run_sqllog_phase(&specific, &runners, &dminit).await?;
    phases::run_verify_phase(&runners, &dminit).await?;
    phases::run_read_routing_phase(&specific, &runners, &dminit).await?;

    crate::cluster::checkpoint::ClusterCheckpoint::remove()?;
    tracing::info!("集群部署完成");
    Ok(())
}
```

### Anti-Patterns to Avoid

- **不调用 `deploy::configure_read_only_standby()`**：`alter database open read only` 由 dmwatcher 自动执行（D-06）。在 run_read_routing_phase 中调用此函数会与 dmwatcher 竞争，导致不可预期错误。
- **不用 `deploy::verify_node_role` 直接替代**：`verify_node_role` 对备节点只检查 `MODE$=STANDBY`，不检查 `STATUS$=OPEN`（见 `deploy.rs:427-433`），且没有重试循环——不满足 D-08/D-09。
- **不在函数内部打 checkpoint**：checkpoint gate 在 `rws/mod.rs` 中管理（D-03），phases.rs 的函数只做执行，不感知 checkpoint。
- **函数超过 40 行**：必须将 poll 逻辑提取到 `wait_for_standby_open`，否则 run_read_routing_phase 超过限制。

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| checkpoint JSON 序列化 | 手写文件格式 | `serde_json::to_string_pretty` + `serde::Serialize` | 已有完整参考实现 `standalone/checkpoint.rs` |
| 轮询超时控制 | 手写计时器 | `tokio::time::sleep` 循环计数（24 次 × 5s = 120s） | 简单直接；`tokio::time::timeout` 也可但会增加复杂度 |
| SSH 命令执行 | 直接调用 russh | `runner.exec(&cmd)` trait 方法 | MockRunner 已实现，测试无需真实 SSH |
| disql V$INSTANCE 查询 | 自定义查询协议 | 复用 `deploy.rs:395` 的 disql 命令格式 | 已验证可用，输出格式已知 |

**Key insight:** 本 phase 几乎所有构建块都已存在，主要工作是组合而非创建。

---

## Common Pitfalls

### Pitfall 1: verify_node_role 不检查 STATUS$=OPEN
**What goes wrong:** 直接调用 `deploy::verify_node_role(standby_node, dminit, NodeRole::Standby, runner)` 看似等价，但该函数对备节点只断言 `output.contains("STANDBY")`，不断言 `STATUS$=OPEN`（见 `deploy.rs:427-433`）。且它没有重试循环。
**Why it happens:** verify_node_role 的设计目标是一次性验证角色，不是等待状态转换。
**How to avoid:** 新建 `wait_for_standby_open`，独立断言 `output.contains("OPEN") && output.contains("STANDBY")`。
**Warning signs:** 部署后备节点处于 MOUNT 状态被当作成功。

### Pitfall 2: MockRunner 默认返回空输出
**What goes wrong:** 测试中 MockRunner 对未预设的命令默认返回 `(vec![], 0)`。空输出不含 "OPEN"，导致 wait_for_standby_open 一直重试直到超时——测试运行 120 秒才失败。
**Why it happens:** `MockRunner::new(vec![])` 的 strict 模式是 false。
**How to avoid:** 为 wait_for_standby_open 的 happy path 测试预设响应：`MockRunner::new(vec![("echo 'SELECT", 0, b"STATUS$   MODE$\nOPEN      STANDBY\n".to_vec())])`；为 timeout 测试不预设响应（MockRunner 默认返回空，会触发重试），但必须将 `MAX_RETRIES` 改为测试参数或使用更小的超时。
**Warning signs:** `#[tokio::test]` 测试运行超过 10 秒。

解决方案：提取常量或参数化重试次数/间隔，方便测试时注入小值。或者为测试路径直接用 `MockRunner::new_strict(vec![...预设响应...])` 只准确匹配。

### Pitfall 3: checkpoint load 时 standby_restore_done=false 导致重复执行
**What goes wrong:** 如果 standby_restore 失败后再成功，但没有设置 `standby_restore_done=true` 就进入后续 phases，下次重跑会重复执行 standby_restore（危险：dmrman restore 需要库处于干净状态）。
**Why it happens:** checkpoint 设置逻辑在 `cp.save()` 之前崩溃（如 install 成功但 save 失败）。
**How to avoid:** 先设置标志再 save：`cp.standby_restore_done = true; cp.save()?;`——即使 save 失败（权限问题），下次重跑会重复该 phase，但不会静默跳过。这与 standalone 相同的权衡。

### Pitfall 4: tracing 标志从 [11/11] 变为 [12/12]
**What goes wrong:** 现有 run_verify_phase 用 `[cluster][11/11]`，新增 run_read_routing_phase 应该是 `[cluster][12/12]`。如果忘记更新其他 phase 的计数，用户看到的 11/11 不再是最后一步，容易困惑。
**Why it happens:** phase 编号硬编码在字符串中。
**How to avoid:** CONTEXT.md D-13 已明确：run_read_routing_phase 用 `[cluster][12/12]`。无需更新其他 phase 编号（它们显示的是自己的步骤序号，不需全部改）。

---

## Code Examples

### V$INSTANCE disql 命令（参考 deploy.rs:402-405）

```rust
// [VERIFIED: src/cluster/deploy.rs:402-405 — 已在项目中使用]
let cmd = format!(
    "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/SYSDBA@localhost:{}",
    shell_quote(&dminit.install_path),
    dminit.port,
);
```

### tokio::time::sleep 轮询模式（参考 deploy.rs:501-502 的 sqllog 重试）

```rust
// [VERIFIED: src/cluster/deploy.rs:497-503 — configure_sqllog 中的重试模式]
for attempt in 1..=24u32 {
    // ... 执行命令 ...
    if attempt < 24 {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
```

### MockRunner 预设响应（参考 deploy.rs:843-849）

```rust
// [VERIFIED: src/cluster/deploy.rs:843-849 — 现有测试模式]
let runner = MockRunner::new(vec![(
    "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
    0,
    b"STATUS$   MODE$\nOPEN      STANDBY\n".to_vec(),
)]);
```

### serde_json checkpoint roundtrip（参考 standalone/checkpoint.rs:39-54）

```rust
// [VERIFIED: src/standalone/checkpoint.rs:39-54]
pub(crate) fn save_to(&self, dir: &Path) -> Result<()> {
    let path = dir.join(FILE_NAME);
    let content = serde_json::to_string_pretty(self)?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub(crate) fn load_from(dir: &Path) -> Result<Option<Self>> {
    let path = dir.join(FILE_NAME);
    if !path.exists() { return Ok(None); }
    let content = std::fs::read_to_string(&path)?;
    match serde_json::from_str(&content) {
        Ok(c) => Ok(Some(c)),
        Err(e) => { tracing::warn!("检查点文件格式错误，忽略: {}", e); Ok(None) }
    }
}
```

---

## Runtime State Inventory

> 本 phase 不涉及重命名/迁移。但 checkpoint 本身会产生运行时状态，需要在 PLAN.md 中说明。

| Category | Items Found | Action Required |
|----------|-------------|-----------------|
| 存储数据 | `dm_cluster_checkpoint.json`（由本 phase 新增写入） | 成功完成后自动删除（D-04） |
| 现有 `dm_installer_checkpoint.json` | standalone checkpoint（无关联） | None — 不同文件名，不冲突 |
| Live service config | dmwatcher/dmmonitor（现有 phase 管理） | None — 本 phase 不新增服务 |
| OS-registered state | None — 不新增 systemd 服务 | None |
| Secrets/env vars | None | None |
| Build artifacts | None | None |

---

## State of the Art

| 现有实现 | Phase 5 变更 | Impact |
|---------|-------------|--------|
| `rws/mod.rs:50` — TODO 注释 | 替换为 `phases::run_read_routing_phase` 调用 | 补完端到端流程 |
| `phases.rs` — 11 个 phase 函数 | 追加第 12 个：`run_read_routing_phase` | 新增只读就绪验证 |
| 无集群 checkpoint | 新建 `cluster/checkpoint.rs` | 支持中断重跑 |
| `deploy::verify_node_role` 不重试 | 新建 `wait_for_standby_open` 带重试 | 区别：等待 vs 一次性验证 |

**Deprecated/outdated:**
- `rws/mod.rs:50` TODO 注释：本 phase 替换，不再保留

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | dmwatcher 启动后，备节点从 MOUNT 自动转换到 STATUS$=OPEN，无需安装器执行额外 SQL | Architecture Patterns / Anti-Patterns | 若 dmwatcher 不自动转换，run_read_routing_phase 会超时失败；需手动调用 configure_read_only_standby |
| A2 | ClusterCheckpoint 不需要 `install_path` 匹配键（standalone 有此键用于防止目录混用） | Architecture Patterns Pattern 1 | 若用户在多个不同集群配置目录共用同一 CWD，可能误读旧 checkpoint；但实际上 rws.toml 和 checkpoint 同目录，用户不会混淆 |
| A3 | 测试中 wait_for_standby_open 可通过 MockRunner 预设响应直接测试，无需真实 dmwatcher 环境 | Validation Architecture | 若 disql 输出格式与预设不匹配（如列顺序不同），测试可能误判 |
| A4 | `specific` 参数在 run_read_routing_phase 中实际不使用（只读节点从 runners 读取） | Architecture Patterns Pattern 3 | 若未来 specific 有必需字段，需同步更新；当前锁定签名（D-11）包含该参数，不影响编译 |

---

## Open Questions

1. **wait_for_standby_open 的超时测试策略**
   - What we know: 24 次 × 5s = 120s 等待在 CI 中不可接受
   - What's unclear: 是否应将 MAX_RETRIES 和 INTERVAL_SECS 提取为 const，并在测试时通过参数化 or 函数变体控制
   - Recommendation: 将轮询参数提取为 const 常量（`const MAX_RETRIES: u32 = 24; const POLL_INTERVAL_SECS: u64 = 5`），单元测试直接用 MockRunner 预设成功响应（happy path 1 次就通过，超时测试提供永远返回 MOUNT 的响应并在外层用 `tokio::time::timeout` 限制测试时间）

2. **cluster/checkpoint.rs 在 cluster/mod.rs 中的 pub 声明**
   - What we know: standalone/mod.rs 直接 `pub mod checkpoint;`
   - What's unclear: `src/cluster/mod.rs` 当前是否存在，需要确认
   - Recommendation: 检查 `src/cluster/mod.rs` 是否已有模块声明，追加 `pub mod checkpoint;`

---

## Environment Availability

本 phase 为纯代码变更，不新增外部依赖或服务。运行时依赖（disql、dmwatcher）由现有节点提供，不在控制机检查。

**Step 2.6: SKIPPED（无新增外部 CLI 或服务依赖）**

---

## Validation Architecture

`workflow.nyquist_validation` 为 `true`，需要完整测试映射。

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` / `cargo nextest` |
| Config file | `Cargo.toml` (workspace) |
| Quick run command | `cargo test -p dm-installer cluster::checkpoint` |
| Full suite command | `cargo test -p dm-installer` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| RWS-01 | `ClusterCheckpoint` save/load/remove roundtrip | unit | `cargo test cluster::checkpoint::tests::test_roundtrip` | ❌ Wave 0 |
| RWS-01 | load 返回 None 当文件不存在 | unit | `cargo test cluster::checkpoint::tests::test_load_returns_none` | ❌ Wave 0 |
| RWS-01 | remove 删除文件 | unit | `cargo test cluster::checkpoint::tests::test_remove_deletes_file` | ❌ Wave 0 |
| RWS-01 | 损坏文件被忽略（返回 None） | unit | `cargo test cluster::checkpoint::tests::test_load_ignores_corrupt` | ❌ Wave 0 |
| RWS-01 | `run_read_routing_phase` happy path — 备节点立即 OPEN | unit | `cargo test cluster::phases::tests::test_run_read_routing_phase_success` | ❌ Wave 0 |
| RWS-01 | `run_read_routing_phase` 超时 path — 备节点始终 MOUNT | unit | `cargo test cluster::phases::tests::test_run_read_routing_phase_timeout` | ❌ Wave 0 |
| RWS-01 | `run_read_routing_phase` 无 read_only 节点时跳过 | unit | `cargo test cluster::phases::tests::test_run_read_routing_phase_no_readonly` | ❌ Wave 0 |
| RWS-02 | 最终状态包含 MODE$=STANDBY 且 STATUS$=OPEN | unit | 包含在 happy path 测试的断言中 | ❌ Wave 0 |
| RWS-01 | rws::run_with_runners checkpoint gate：已完成的 phase 不重复执行 | integration | `cargo test cluster::rws::tests::test_checkpoint_skips_completed` | ❌ Wave 0 |

**手动验证（不自动化）：**
- 真实双节点 RWS 部署端到端测试（依赖真实达梦环境）
- 中断后重跑验证（需要在真实 SSH 节点模拟失败）

### Sampling Rate

- **Per task commit:** `cargo test -p dm-installer cluster::checkpoint cluster::phases`
- **Per wave merge:** `cargo test -p dm-installer`
- **Phase gate:** 全套 green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src/cluster/checkpoint.rs` — 新建文件 + 内嵌测试模块（covers RWS-01 checkpoint 行为）
- [ ] `src/cluster/phases.rs` 内 `#[cfg(test)]` 补充 `run_read_routing_phase` 测试 3 个（covers RWS-01/02）
- [ ] `src/cluster/rws/mod.rs` 内 `#[cfg(test)]` 补充 checkpoint gate 集成测试 1 个

*(现有 test 基础设施（MockRunner, deploy tests）已完整，无需新建框架文件)*

---

## Security Domain

本 phase 不引入新的认证、会话、输入处理或加密组件。checkpoint 文件仅含布尔标志，无敏感信息。SQL 命令与现有 `deploy::verify_node_role` 完全相同，shell_quote 保护已覆盖。

**ASVS 适用性：** 与现有 SSH 执行模式相同，无新增攻击面。略。

---

## Sources

### Primary (HIGH confidence)
- `src/standalone/checkpoint.rs` — save/load/remove 完整实现，直接复用模式 [VERIFIED: 本地代码]
- `src/cluster/deploy.rs:395-436` — `verify_node_role` V$INSTANCE 查询格式 [VERIFIED: 本地代码]
- `src/cluster/deploy.rs:486-503` — `configure_sqllog` 重试循环模式 [VERIFIED: 本地代码]
- `src/cluster/phases.rs` — 所有现有 phase 函数签名和模式 [VERIFIED: 本地代码]
- `src/cluster/rws/mod.rs` — run_with_runners 结构和 TODO:50 位置 [VERIFIED: 本地代码]
- `src/config/cluster.rs:186-210` — NodeConfig.read_only 字段定义 [VERIFIED: 本地代码]
- `src/common/ssh/mock.rs` — MockRunner API 和预设响应格式 [VERIFIED: 本地代码]
- `.planning/phases/05-rws/05-CONTEXT.md` — 所有 D-01 ~ D-13 决策 [VERIFIED: 本地规划文档]

### Secondary (MEDIUM confidence)
- `tokio::time::sleep` 文档 — 异步 sleep 语义 [ASSUMED: 训练数据，tokio 1.x stable API 不变]

---

## Metadata

**Confidence breakdown:**
- Standard Stack: HIGH — 无新依赖，完全复用已验证 crate
- Architecture: HIGH — 参考实现在代码库中完整存在
- Pitfalls: HIGH — 从现有代码逻辑推导，尤其 verify_node_role 对备节点不检查 STATUS=OPEN 已通过直读代码确认
- 测试策略: MEDIUM — timeout 测试的具体实现方式待实施时确定（参见 Open Question 1）

**Research date:** 2026-06-14
**Valid until:** 2026-07-14（稳定 Rust 生态，30 天有效期）
