---
phase: 07-dsc
reviewed: 2026-06-15T06:30:00Z
depth: standard
files_reviewed: 7
files_reviewed_list:
  - src/cluster/checkpoint.rs
  - src/cluster/dsc/deploy.rs
  - src/cluster/dsc/mod.rs
  - src/cluster/dsc/templates.rs
  - src/config/cluster.rs
  - src/cluster/primary_standby/mod.rs
  - src/cluster/rws/mod.rs
findings:
  critical: 3
  warning: 5
  info: 3
  total: 11
status: issues_found
---

# Phase 07: Code Review Report

**Reviewed:** 2026-06-15T06:30:00Z
**Depth:** standard
**Files Reviewed:** 7
**Status:** issues_found

## Summary

本次审查覆盖 Phase 07 DSC 共享存储集群实现的全部 7 个源文件。整体架构清晰，checkpoint gate 设计合理，shell 注入防御（`shell_quote`）覆盖了大多数路径。但发现 **3 个 BLOCKER 级错误**，均涉及端口号计算逻辑不正确，会导致多节点集群部署失败。另有 5 个警告和 3 个 Info 级问题。

---

## Critical Issues

### CR-01: `verify_dsc_node` 始终使用 `dminit.port`（5236），无法验证非主节点

**File:** `src/cluster/dsc/deploy.rs:397-401`

**Issue:** `verify_dsc_node` 通过 `disql` 连接 `localhost:{dminit.port}` 验证节点状态。但 DSC 架构中，各节点的 dmserver 端口在 `dminit.ini` 里按节点数组下标分配（`PORT_NUM = dminit.port + node_index`）。当验证 index > 0 的节点时，函数始终连接 5236（index 0 的端口），而该节点实际监听的是 5237、5238…，导致：

1. 对 standby 节点的验证连接到了 primary 节点的端口，得到误报"验证通过"；
2. 若 primary 未就绪，连接会失败，阻断整个验证流程。

测试（`test_verify_dsc_node_accepts_open_normal`）未能覆盖此问题，因为 MockRunner 匹配的是命令前缀而非实际端口值。

**Fix:**

```rust
// 在 run_start_and_verify_dmserver_all_nodes 中，为 verify_dsc_node 传入 per-node port
// 方案一：为 verify_dsc_node 增加 port 参数
pub async fn verify_dsc_node(
    node: &NodeConfig,
    dminit: &DminitConfig,
    node_port: u16,           // 新增
    runner: &dyn CommandRunner,
) -> Result<()> {
    let cmd = format!(
        "echo 'SELECT STATUS$, MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
        shell_quote(&dminit.install_path),
        shell_quote(&dminit.sysdba_password),
        node_port,            // 使用传入的 per-node port
    );
    // ...
}

// 调用侧（mod.rs run_start_and_verify_dmserver_all_nodes）:
let verify_futs: Vec<_> = runners
    .iter()
    .enumerate()
    .map(|(node_idx, (node, runner))| {
        let node_port = dminit.port.saturating_add(node_idx as u16);
        async move {
            deploy::verify_dsc_node(&node, &dminit, node_port, runner.as_ref()).await
        }
    })
    .collect();
```

---

### CR-02: DMASM 健康检查始终轮询端口 9349，忽略各节点实际端口

**File:** `src/cluster/dsc/mod.rs:271-275`

**Issue:** `run_start_css_asm_all_nodes` 在所有节点上并行启动 DMASM 后，对每个节点都检查端口 9349。但 `dmdcr_cfg.ini` 中各节点的 DMASM 端口按 `9349 + node_index * 2` 递增：节点 0 用 9349，节点 1 用 9351，节点 2 用 9353……

结果：对节点 1 的健康检查实际上连接的是节点 0 的 9349 端口。若节点 0 的 DMASM 已就绪，节点 1、2 等节点的检查会立即通过，即使这些节点的 DMASM 还未启动，从而导致 Gate 5（`run_asm_init_first_node`）在其余节点 DMASM 未就绪时就开始执行。

**Fix:**

```rust
// run_start_css_asm_all_nodes 末尾改为按节点 index 计算端口
for (node_idx, (node, _)) in runners.iter().enumerate() {
    let asm_port = 9349u16 + (node_idx as u16) * 2;  // 与 dmdcr_cfg.ini 保持一致
    tracing::info!("[node:{}] 等待 DMASM 端口 {} 就绪...", node.host, asm_port);
    health_check_fn(node.host.clone(), asm_port, 60).await?;
}
```

---

### CR-03: `first_node`（Primary）的 dmserver 健康检查端口错误

**File:** `src/cluster/dsc/mod.rs:362`

**Issue:** `run_start_and_verify_dmserver_all_nodes` 中，first_node 启动 dmserver 后执行：

```rust
health_check_fn(first_node.host.clone(), dminit.port, 60).await?;
```

但 `dminit.ini` 中各节点的端口是按**数组下标**（`node_index`，由 `enumerate()` 决定）分配的，与节点角色（Primary/Standby）无关。若 Primary 节点在 `runners` 数组中处于非零位置（如 index 1），则其实际端口为 `dminit.port + 1 = 5237`，但健康检查却连接 5236，导致等待超时或误判。

**Fix:**

```rust
let (first_node, first_runner) = &runners[first_idx];
let first_node_port = dminit.port.saturating_add(first_idx as u16);  // 按 index 计算
// ...
health_check_fn(first_node.host.clone(), first_node_port, 60).await?;
```

---

## Warnings

### WR-01: 三个函数超出 40 行限制（违反项目约定）

**File:** `src/cluster/dsc/mod.rs`

**Issue:** CLAUDE.md 明确要求函数不超过 40 行：

| 函数 | 行数 |
|------|------|
| `run_with_runners` | 105 行 |
| `run_start_and_verify_dmserver_all_nodes` | 60 行 |
| `run_start_css_asm_all_nodes` | 51 行 |

`distribute_config_dir`（`deploy.rs`）也有 75 行。

**Fix:** 将 `run_start_and_verify_dmserver_all_nodes` 中的"先启动 first_node"和"再启动 other_nodes"拆成两个独立私有函数；`run_with_runners` 的 8 个 gate 可提取为 `run_all_gates` 并将各 gate 的日志/分派逻辑封装在内部。

---

### WR-02: `distribute_config_dir` 的 `first_node_index` 参数未实际使用

**File:** `src/cluster/dsc/deploy.rs:312-386`

**Issue:** 函数签名接收 `first_node_index: usize`，但函数体最后用 `let _ = first_node_index;` 显式丢弃，注释说"保留对称性"。这是一个签名上的误导：调用者传入该参数后没有任何副作用，也不影响行为。

**Fix:** 移除该参数，或若确实需要记录日志则在函数内实际使用它：

```rust
// 若要记录来源节点：
tracing::debug!("从 node[{}] 分发 dsc{} config 目录", first_node_index, other_node_index);
// 并移除末尾的 `let _ = first_node_index;`
```

---

### WR-03: `first_node_index` 不必要地声明为 `async`

**File:** `src/cluster/dsc/mod.rs:153-158`

**Issue:** 函数体仅做 `Iterator::position()` 查找，不含任何 `await` 点，声明 `async` 会创建不必要的 future 对象，且会误导读者以为存在异步 I/O。

**Fix:**

```rust
fn first_node_index(runners: &phases::Runners) -> Result<usize> {
    runners
        .iter()
        .position(|(n, _)| n.role == NodeRole::Primary)
        .ok_or_else(|| anyhow::anyhow!("DSC 集群缺少 primary 节点（first_node）"))
}
// 调用侧去掉 .await
let first_idx = first_node_index(runners)?;
```

---

### WR-04: 测试的 `set_current_dir` 恢复不具备 panic 安全性

**File:** `src/cluster/dsc/mod.rs:513-538` 及同类代码块

**Issue:** 5 个集成测试均模式为：

```rust
std::env::set_current_dir(dir.path()).unwrap();
// ... 可能 panic 的代码 ...
std::env::set_current_dir(original_dir).unwrap();  // 如果上面 panic 则永远不会执行
```

若中间代码 panic（如断言失败），`original_dir` 的恢复不会执行。由于 `CWD_LOCK` 为全局静态锁，后续测试虽然会序列化执行，但工作目录已指向一个被 `TempDir::drop` 删除的路径，导致后续测试的 checkpoint 文件操作失败。

**Fix:** 使用 RAII guard 或 `defer` 模式：

```rust
struct CwdGuard(std::path::PathBuf);
impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}
// 使用：
let _cwd_guard = CwdGuard(std::env::current_dir().unwrap());
std::env::set_current_dir(dir.path()).unwrap();
// 之后无需手动恢复，Drop 自动处理（包括 panic 情况）
```

---

### WR-05: DSC 验证未要求最少 2 个节点

**File:** `src/config/cluster.rs:327-339`

**Issue:** `validate_dsc` 调用 `check_nodes_not_empty`（要求 >= 1 个节点），但 DSC 共享存储集群本质上需要至少 2 个节点（1 Primary + 1 Standby）才有意义。单节点 DSC 配置会通过验证，然后在部署时在 Gate 7（`run_distribute_config_dirs`）的 for 循环中静默跳过（`if other_idx == first_idx { continue }`），不产生任何错误，形成误导性的"成功"部署。

**Fix:**

```rust
fn validate_dsc(cfg: &ClusterSpecificConfig) -> Result<()> {
    check_nodes_not_empty(cfg)?;
    // 新增：DSC 需要至少 2 个节点
    let non_monitor_count = cfg.nodes.iter()
        .filter(|n| n.role != NodeRole::Monitor)
        .count();
    if non_monitor_count < 2 {
        bail!("配置验证失败: DSC 集群至少需要 2 个节点（1 primary + 1 standby）");
    }
    // ...其余校验
}
```

---

## Info

### IN-01: `#![allow(dead_code)]` 在 Plan 04 完成后应移除

**File:** `src/cluster/dsc/deploy.rs:2` 和 `src/cluster/dsc/templates.rs:2`

**Issue:** 两个文件头部均有文件级 `#![allow(dead_code)]`，注释说明是 Plan 03/Plan 04 引用之前的临时措施。Plan 04 已完成，`mod.rs` 现在通过 `deploy::` 前缀引用了 `deploy.rs` 的全部公开函数，`deploy.rs` 也通过 `crate::cluster::dsc::templates::` 引用了 `templates.rs`。这两个属性已过时，保留会掩盖将来真正未使用的函数。

**Fix:** 移除两个文件的 `#![allow(dead_code)]` 并执行 `cargo build` 验证无警告。

---

### IN-02: `dminit.ini` 中 `SYSDBA_PWD` 以明文写入远程磁盘

**File:** `src/cluster/dsc/templates.rs:126-131`

**Issue:** `generate_dminit_ini` 将 `dminit.sysdba_password`（通常为 `SYSDBA` 或用户自定义密码）以明文写入 `dminit.ini` 文件，然后通过 SFTP 上传到远程节点。文件权限取决于远程主机的 umask，若未正确设置，可能被其他用户读取。这是 DM 工具链的已知限制，但应在文档或注释中说明。

**Fix:** 在代码注释和用户文档中明确说明此行为；建议用户在部署后删除 `dminit.ini` 或限制其文件权限（`chmod 600`）。可在 `run_dminit_shared` 的 dminit 执行成功后添加自动删除步骤：

```rust
// dminit 执行成功后清理含密码的 ini 文件
if let Err(e) = runner.exec(&format!("rm -f {}", shell_quote(&dminit_ini_path))).await {
    tracing::warn!("清理 dminit.ini 失败（含明文密码），请手动删除: {}", e);
}
```

---

### IN-03: DSC 配置允许 `Monitor` 角色节点但部署逻辑忽略它

**File:** `src/config/cluster.rs:372-374`

**Issue:** `validate_dsc` 复用 `check_role_uniqueness`，允许最多一个 `Monitor` 角色节点。但 DSC 架构不使用 `dmwatcher`/`dmmonitor`，`Monitor` 节点在 DSC 部署流程中会被当成普通节点处理（建立 SSH 连接、安装软件包、分发配置、启动 DMCSS/DMASM/dmserver），这与 Monitor 节点的预期角色不符，可能导致意外的配置状态。

**Fix:** 在 `validate_dsc` 中添加明确拒绝 `Monitor` 角色的校验：

```rust
let has_monitor = cfg.nodes.iter().any(|n| n.role == NodeRole::Monitor);
if has_monitor {
    bail!("配置验证失败: DSC 集群不支持 monitor 角色节点，请仅配置 primary/standby");
}
```

---

## 安全问题汇总

| 安全关注点 | 位置 | 状态 |
|---------|------|------|
| Shell 注入（磁盘路径/安装路径） | `deploy.rs` | 已通过 `shell_quote` 防御 |
| Shell 注入（SYSDBA 密码） | `deploy.rs:400` | 已通过 `shell_quote` 防御 |
| SYSDBA 密码明文写入磁盘 | `templates.rs:126` | IN-02，建议事后清理 |
| disql 连接串密码特殊字符（`@`, `:`）| `deploy.rs:398-401` | 低风险：`shell_quote` 防止了 shell 注入；但密码中含 `@`/`:` 会破坏 disql 连接串解析，应在文档中说明不支持含这些字符的密码 |
| tar 包内容验证 | `deploy.rs:343-350` | 可接受：tar 来源是 first_node（已受信），在可信内网传输 |

---

_Reviewed: 2026-06-15T06:30:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
