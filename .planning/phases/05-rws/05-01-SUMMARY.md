---
phase: 05-rws
plan: "01"
subsystem: cluster-checkpoint
tags: [checkpoint, persistence, idempotency, rws]
dependency_graph:
  requires: []
  provides: [cluster::checkpoint::ClusterCheckpoint]
  affects: [src/cluster/rws/mod.rs]
tech_stack:
  added: []
  patterns: [serde_json roundtrip, TempDir 测试隔离, Ok(None) 容错]
key_files:
  created:
    - src/cluster/checkpoint.rs
  modified:
    - src/cluster/mod.rs
decisions:
  - "ClusterCheckpoint 不含 install_path 匹配键（与 standalone::Checkpoint 的关键差异），因为 rws.toml 和 checkpoint 同目录不会混淆"
  - "使用 Default derive 代替 new() 构造函数，调用方用 unwrap_or_default()"
  - "load_from 单参数签名（无 install_path 第二参数），简化调用方代码"
metrics:
  duration_seconds: 104
  completed: "2026-06-14T11:55:10Z"
  tasks_completed: 1
  tasks_total: 1
  files_created: 1
  files_modified: 1
---

# Phase 5 Plan 01: ClusterCheckpoint 持久化模块 Summary

**一句话总结：** 新建 ClusterCheckpoint 结构体，通过 serde_json 将 5 个布尔字段序列化到磁盘，支持集群安装中断重跑（损坏文件静默忽略）。

## 执行结果

### 任务完成情况

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | 新建 src/cluster/checkpoint.rs 并声明模块 | 4a41b18 | src/cluster/checkpoint.rs (新建), src/cluster/mod.rs (+1 行) |

### 实现细节

**结构体字段（5 个布尔，全部 `#[serde(default)]`）：**
- `preflight_done` — 预检阶段完成标志
- `install_done` — DM 安装包安装完成标志
- `primary_init_done` — 主库初始化（dminit）完成标志
- `backup_done` — 主库备份完成标志
- `standby_restore_done` — 备库 dmrman restore 完成标志

**公共代理方法（指向当前工作目录）：**
- `save(&self) -> Result<()>` — 调用 `save_to(&cwd())`
- `load() -> Result<Option<Self>>` — 调用 `load_from(&cwd())`
- `remove() -> Result<()>` — 调用 `remove_from(&cwd())`

**与 `standalone::Checkpoint` 的关键差异：**

| 特性 | standalone::Checkpoint | ClusterCheckpoint |
|------|----------------------|-------------------|
| install_path 匹配键 | 有（防止目录混淆） | 无（per A-02，同目录无混淆风险）|
| 构造函数 | `new(install_path, ...)` | `Default::default()` |
| load_from 签名 | `load_from(dir, install_path)` | `load_from(dir)` |
| 字段类型 | 密码 + 路径 + bool | 纯 bool |

### 测试结果

```
test cluster::checkpoint::tests::test_load_returns_none ... ok
test cluster::checkpoint::tests::test_load_ignores_corrupt ... ok
test cluster::checkpoint::tests::test_remove_deletes_file ... ok
test cluster::checkpoint::tests::test_roundtrip ... ok

test result: ok. 4 passed; 0 failed; 0 ignored
```

### 威胁缓解验证

| Threat | Mitigation | Verified |
|--------|-----------|---------|
| T-05-03 | corrupt JSON -> warn + Ok(None) | test_load_ignores_corrupt PASSED |

## Deviations from Plan

None - 计划按原样执行，实现与 `<action>` 描述完全一致。

## Known Stubs

None - 所有方法有完整实现，无占位符。

## Threat Flags

None - 无新增安全相关入口点，仅操作本地文件（布尔标志）。

## Self-Check

- [x] `src/cluster/checkpoint.rs` 存在 — FOUND
- [x] `src/cluster/mod.rs` 含 `pub mod checkpoint;` — FOUND
- [x] 提交 4a41b18 存在 — FOUND
- [x] `cargo test cluster::checkpoint::tests` — 4 PASSED
- [x] `cargo build` — 退出码 0

## Self-Check: PASSED
