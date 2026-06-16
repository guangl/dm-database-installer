---
phase: 5
slug: rws
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-14
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` / `cargo nextest` |
| **Config file** | `Cargo.toml` (workspace) |
| **Quick run command** | `cargo test -p dm-installer cluster::checkpoint cluster::phases` |
| **Full suite command** | `cargo test -p dm-installer` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p dm-installer cluster::checkpoint cluster::phases`
- **After every plan wave:** Run `cargo test -p dm-installer`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** ~10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| checkpoint-01 | 01 | 1 | RWS-01 | — | checkpoint 文件含布尔标志，无敏感数据 | unit | `cargo test cluster::checkpoint::tests::test_roundtrip` | ❌ Wave 0 | ⬜ pending |
| checkpoint-02 | 01 | 1 | RWS-01 | — | N/A | unit | `cargo test cluster::checkpoint::tests::test_load_returns_none` | ❌ Wave 0 | ⬜ pending |
| checkpoint-03 | 01 | 1 | RWS-01 | — | N/A | unit | `cargo test cluster::checkpoint::tests::test_remove_deletes_file` | ❌ Wave 0 | ⬜ pending |
| checkpoint-04 | 01 | 1 | RWS-01 | — | 损坏文件不崩溃 | unit | `cargo test cluster::checkpoint::tests::test_load_ignores_corrupt` | ❌ Wave 0 | ⬜ pending |
| routing-01 | 02 | 2 | RWS-01, RWS-02 | — | 不执行写 SQL | unit | `cargo test cluster::phases::tests::test_run_read_routing_phase_success` | ❌ Wave 0 | ⬜ pending |
| routing-02 | 02 | 2 | RWS-01 | — | 超时返回 Error | unit | `cargo test cluster::phases::tests::test_run_read_routing_phase_timeout` | ❌ Wave 0 | ⬜ pending |
| routing-03 | 02 | 2 | RWS-01 | — | 无只读节点时跳过 | unit | `cargo test cluster::phases::tests::test_run_read_routing_phase_no_readonly` | ❌ Wave 0 | ⬜ pending |
| integration-01 | 03 | 3 | RWS-01 | — | 已完成步骤不重复 | integration | `cargo test cluster::rws::tests::test_checkpoint_skips_completed` | ❌ Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/cluster/checkpoint.rs` — 新建文件，含内嵌测试模块（covers RWS-01 checkpoint 行为）
- [ ] `src/cluster/phases.rs` 内 `#[cfg(test)]` 补充 `run_read_routing_phase` 测试 3 个（covers RWS-01/02）
- [ ] `src/cluster/rws/mod.rs` 内 `#[cfg(test)]` 补充 checkpoint gate 集成测试 1 个

*现有测试基础设施（MockRunner, deploy tests）已完整，无需新建框架文件*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| 真实双节点 RWS 端到端部署 | RWS-01, RWS-02 | 依赖真实达梦环境和 SSH 节点 | 配置 rws.toml，执行 `dm-installer install rws`，验证两节点均启动 |
| 中断后重跑从 checkpoint 恢复 | RWS-01 | 需模拟真实节点 SSH 失败 | 在 backup 完成后杀进程，重跑应从 standby_restore 继续 |
