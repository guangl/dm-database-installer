---
phase: 05-rws
plan: "03"
subsystem: cluster/rws
tags: [checkpoint, gate, idempotency, rws, read-routing]
dependency_graph:
  requires:
    - "05-01: ClusterCheckpoint CRUD（save/load/remove）"
    - "05-02: run_read_routing_phase 实现"
  provides:
    - "run_with_runners: 含五个 checkpoint gate 的完整集群部署流程"
  affects:
    - "src/cluster/rws/mod.rs"
tech_stack:
  added: []
  patterns:
    - "辅助函数拆分（run_early_checkpoints + run_init_restore_checkpoints）：规避 40 行约束，同时按参数依赖分组"
    - "泛型约束传递（where F: Fn(...)）：避免 dyn trait 对象大小问题"
    - "checkpoint gate 模式：load → 条件跳过 → 执行 → save"
key_files:
  created: []
  modified:
    - src/cluster/rws/mod.rs
decisions:
  - "提取两个辅助函数（run_early_checkpoints / run_init_restore_checkpoints）而不是单个 run_checkpointed_phases：按参数依赖自然分组（preflight/install 无需 health_check_fn，primary_init 需要），同时保持各函数 ≤40 行"
  - "使用泛型参数 <F> 而非 &dyn HealthCheckFn：避免 DST unsized 编译错误，与 Rust 惯用法一致"
metrics:
  duration_seconds: 220
  completed: "2026-06-14T12:10:41Z"
  tasks_completed: 1
  tasks_total: 1
  files_created: 0
  files_modified: 1
---

# Phase 5 Plan 03: Checkpoint Gate 集成 Summary

**一句话总结：** 在 `run_with_runners` 中嵌入五个 checkpoint gate（preflight/install/primary_init/backup/standby_restore），使 `dm-installer install rws` 支持中断重跑不重复执行危险操作，全部完成后删除 checkpoint 文件。

## 执行结果

### 任务完成情况

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 (RED) | 添加 test_checkpoint_gate_skips_done_phases | 0eecafd | src/cluster/rws/mod.rs (+26 行) |
| 1 (GREEN) | 嵌入五个 checkpoint gate + 提取辅助函数 | 4740082 | src/cluster/rws/mod.rs (+69 -9 行) |

### 实现细节

**函数结构（所有函数满足 ≤40 行约束）：**

| 函数 | 行数 | 职责 |
|------|------|------|
| `run_with_runners` | 26 行 | 入口：加载 cp，调用辅助函数，执行后续 phases，删除 cp |
| `run_early_checkpoints` | 22 行 | gate 1 preflight + gate 2 install |
| `run_init_restore_checkpoints` | 34 行 | gate 3 primary_init + gate 4 backup + gate 5 standby_restore |

**checkpoint gate 模式（五个 gate）：**
```rust
if !cp.xxx_done {
    phases::run_xxx_phase(...).await?;
    cp.xxx_done = true;
    cp.save()?;
} else {
    tracing::info!("[续] 跳过 xxx（checkpoint）");
}
```

**调用顺序（per D-02 / D-03 / D-13）：**
- Gate 1: `run_preflight` → `cp.preflight_done = true; cp.save()?`
- Gate 2: `run_install_phase` → `cp.install_done = true; cp.save()?`
- Gate 3: `run_primary_init_phase` → `cp.primary_init_done = true; cp.save()?`
- Gate 4: `run_backup_phase` → `cp.backup_done = true; cp.save()?`
- Gate 5: `run_standby_restore_phase` → `cp.standby_restore_done = true; cp.save()?`
- 直接执行（不打点）：distribute → startup → watcher → monitor → sqllog → verify → **read_routing**
- 完成：`ClusterCheckpoint::remove()?`（per D-04）

### 测试结果

```
test cluster::rws::tests::test_checkpoint_gate_skips_done_phases ... ok

test result: ok. 1 passed; 0 failed; 0 ignored
```

**全套测试：** 183 passed; 0 failed

### 最终验证

| 检查项 | 结果 |
|--------|------|
| `grep "run_read_routing_phase" src/cluster/rws/mod.rs` | FOUND |
| `grep "TODO" src/cluster/rws/mod.rs` | 空（TODO 已全部删除）|
| `grep "ClusterCheckpoint::remove" src/cluster/rws/mod.rs` | FOUND |
| `cargo test cluster::rws::tests` | 1 PASSED |
| `cargo test` 全套 | 183 PASSED; 0 FAILED |
| `cargo build` | 退出码 0 |

## Deviations from Plan

### 自动修复的问题

**1. [Rule 1 - Bug] run_checkpointed_phases 单一辅助函数导致 46 行，超过约束**
- **发现于：** Task 1 实现阶段，写完后统计行数
- **问题：** PLAN.md 提到如超 40 行需提取辅助函数，但用 `run_checkpointed_phases` 单个函数时含 5 个 gate（每个 7 行含 else）+ 签名约 6 行 = 46 行
- **修复：** 按参数依赖分为两个辅助函数：`run_early_checkpoints`（无 health_check_fn）和 `run_init_restore_checkpoints`（含 health_check_fn 泛型），最大 34 行
- **文件：** src/cluster/rws/mod.rs

**2. [Rule 1 - Bug] &dyn HealthCheckFn 引起 DST unsized 编译错误**
- **发现于：** 第一版使用 `type HealthCheckFn = dyn Fn(...)` 别名时
- **问题：** `&HealthCheckFn` 是 `&dyn Fn(...)` 即 DST，不能作为函数参数直接使用，编译器报 `E0310` 和 `E0277`
- **修复：** 改用泛型参数 `<F> where F: Fn(...)` 传递，与 `run_with_runners` 原有模式一致
- **文件：** src/cluster/rws/mod.rs

## Known Stubs

无。checkpoint gate 完整实现，无硬编码空值或 placeholder。

## Threat Flags

无新增安全相关入口点。checkpoint gate 仅读写本地 JSON 文件（布尔标志），T-05-07/08/09 均已在 PLAN.md 评估为 accept。

## Self-Check: PASSED

| 检查项 | 结果 |
|--------|------|
| src/cluster/rws/mod.rs 修改存在 | FOUND |
| commit 0eecafd (RED) 存在 | FOUND |
| commit 4740082 (GREEN) 存在 | FOUND |
| test_checkpoint_gate_skips_done_phases PASSED | ok |
| 全套 183 测试 PASSED | ok. 183 passed; 0 failed |
| cargo build 退出码 0 | BUILD OK |
