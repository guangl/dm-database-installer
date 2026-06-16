---
phase: 05-rws
verified: 2026-06-14T12:20:00Z
status: passed
score: 8/8 must-haves verified
overrides_applied: 0
---

# Phase 05: RWS Checkpoint 断点续传 + run_read_routing_phase 验证报告

**Phase Goal:** 为 rws 集群安装实现 checkpoint 断点续传，使五个高代价 phase（preflight/install/primary_init/backup/standby_restore）在每次完成后保存进度，中断后可跳过已完成的步骤重新执行；同时补全 run_read_routing_phase 调用使 rws 安装流程端到端可走通。
**Verified:** 2026-06-14T12:20:00Z
**Status:** PASSED
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|---------|
| 1 | ClusterCheckpoint 可将布尔标志序列化为 JSON 并从磁盘还原 | VERIFIED | `checkpoint.rs` 使用 `serde_json::to_string_pretty` + `serde_json::from_str`；test_roundtrip PASSED |
| 2 | 损坏或缺失的 checkpoint 文件不导致崩溃，返回 None | VERIFIED | `load_from` 文件不存在返回 `Ok(None)`；解析失败 warn+Ok(None)；test_load_returns_none / test_load_ignores_corrupt PASSED |
| 3 | remove() 调用后文件不存在 | VERIFIED | `remove_from` 调用 `fs::remove_file`；test_remove_deletes_file PASSED |
| 4 | save/load/remove 三个公共代理方法指向当前工作目录 | VERIFIED | 三个公共方法各自调用 `&cwd()`，`cwd()` 使用 `std::env::current_dir()` |
| 5 | run_read_routing_phase 遍历 runners，过滤 role==Standby && read_only==true 节点 | VERIFIED | `phases.rs:324` `.filter(|(node, _)| node.role == NodeRole::Standby && node.read_only)` |
| 6 | 对每个只读备节点最多轮询 24 次（间隔 5s），OPEN+STANDBY 时返回 Ok | VERIFIED | `MAX_RETRIES=24`，`POLL_INTERVAL_SECS=5`；test_run_read_routing_phase_success PASSED |
| 7 | 24 次重试后未 OPEN 时返回 Err 含节点 host | VERIFIED | `anyhow::bail!("备节点 {} 未在 …", node.host, …)`；test_run_read_routing_phase_timeout PASSED（含 host 断言）|
| 8 | run_with_runners 含五个 checkpoint gate + run_read_routing_phase 调用 + 完成后 remove() | VERIFIED | `rws/mod.rs:42-52`：load cp、5 个 gate in 辅助函数、run_read_routing_phase 在 run_verify_phase 之后、ClusterCheckpoint::remove() |

**Score:** 8/8 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/cluster/checkpoint.rs` | ClusterCheckpoint + save/load/remove + 4 个测试 | VERIFIED | 123 行；5 个布尔字段全部含 `#[serde(default)]`；4 测试 ok |
| `src/cluster/mod.rs` | 含 `pub mod checkpoint;` 和 `pub mod phases;` | VERIFIED | 第 5 行 `pub mod checkpoint;`，第 10 行 `pub mod phases;` |
| `src/cluster/phases.rs` | wait_for_standby_open + run_read_routing_phase + 3 个测试 | VERIFIED | 407 行；两个函数存在；3 测试 ok |
| `src/cluster/rws/mod.rs` | checkpoint gate 集成 + run_read_routing_phase 调用 | VERIFIED | 139 行；5 个 gate 通过两个辅助函数实现；run_read_routing_phase 在第 51 行；TODO 已删除 |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| `rws/mod.rs::run_with_runners` | `cluster::checkpoint::ClusterCheckpoint` | `ClusterCheckpoint::load()?.unwrap_or_default()` | WIRED | `rws/mod.rs:42` 确认 |
| `run_read_routing_phase` | `wait_for_standby_open` | 串行 for 循环调用 | WIRED | `phases.rs:330-332` 确认 |
| `wait_for_standby_open` | `runner.exec` | disql V$INSTANCE 查询 | WIRED | `phases.rs:282-283`；SQL 含 `SELECT STATUS$,MODE$ FROM V$INSTANCE` |
| `rws/mod.rs` | `phases::run_read_routing_phase` | 直接调用，替换 TODO:50 | WIRED | `rws/mod.rs:51`；`grep "TODO"` 输出为空 |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| checkpoint 4 个单元测试 | `cargo test cluster::checkpoint::tests` | 4 passed; 0 failed | PASS |
| phases 3 个单元测试 | `cargo test cluster::phases::tests` | 3 passed; 0 failed | PASS |
| rws 1 个集成测试 | `cargo test cluster::rws::tests` | 1 passed; 0 failed | PASS |
| 全套测试 | `cargo test` | 183 passed; 0 failed | PASS |
| cargo build | `cargo build` | 退出码 0 | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|---------|
| RWS-01 | 05-01, 05-02, 05-03 | 用户可执行 `dm-installer install rws` 完成读写分离集群完整部署（主库+只读备库，端到端） | SATISFIED | run_with_runners 包含完整流程：preflight→install→primary_init→backup→standby_restore→distribute→startup→watcher→monitor→sqllog→verify→read_routing；全 183 测试 PASSED；cargo build 0 |
| RWS-02 | 05-02, 05-03 | 部署完成后备节点自动通过 SQL 配置为只读模式（READ_ONLY 标志） | SATISFIED（流程侧） | run_read_routing_phase 等待并验证 `STATUS$=OPEN MODE$=STANDBY`；dmwatcher 自动转换无需 `alter database open read only`（per D-06 决策）；代码路径验证备节点到达只读 OPEN 状态后流程才结束 |

备注：RWS-02 的"自动配置只读"通过 dmwatcher 完成（架构决策 D-06），安装器侧的验证逻辑已实现。但 dmwatcher 的实际行为属于运行时验证（需要真实集群环境）。

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/cluster/deploy.rs | 439 | `configure_read_only_standby` dead_code | INFO | 函数存在但未被调用（符合 D-06 决策，Phase 05 禁止调用）；仅为警告非错误 |
| src/common/ssh/mock.rs | 36 | `set_sftp_read` dead_code | INFO | 测试工具类，与本 Phase 实现无关 |

无 TBD / FIXME / XXX / TODO 标记存在于三个修改文件中（grep 验证输出为空）。

### Human Verification Required

无。所有必须人工验证的行为（dmwatcher 实际启动后备节点状态转换）已由架构决策 D-06 明确为运行时行为，不属于安装器代码的验证范围。

---

_Verified: 2026-06-14T12:20:00Z_
_Verifier: Claude (gsd-verifier)_
