---
phase: 06
slug: status
status: approved
nyquist_compliant: true
wave_0_complete: true
created: 2026-06-14
updated: 2026-06-15
---

# Phase 06 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust) |
| **Config file** | none — existing Cargo.toml covers it |
| **Quick run command** | `cargo test status` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test status`
- **After every plan wave:** Run `cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 06-01-01 | 01 | 1 | STAT-01 | T-06-01 | shell_quote 转义 | unit (TDD inline) | `cargo test -p dm-database-installer --lib status::tests` | ✅ inline | ⬜ pending |
| 06-01-02 | 01 | 1 | STAT-02 | T-06-04 | tokio::time::timeout 5s | unit (MockRunner) | `cargo test -p dm-database-installer --lib status::tests` | ✅ inline | ⬜ pending |
| 06-01-03 | 01 | 1 | STAT-03 | — | N/A | unit (TDD inline) | `cargo test -p dm-database-installer --lib status::tests` | ✅ inline | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] `src/status/mod.rs` inline tests — stubs for STAT-01, STAT-02, STAT-03 are created **inline within Task 1** via TDD

**Note on Wave 0 handling:**
Wave 0 is handled **inline in Task 1 via TDD**. Task 1 has `tdd="true"` and an explicit `<behavior>` block listing 7 failing tests that MUST be written FIRST (RED phase), before any production implementation (GREEN phase). The `src/status/mod.rs` test module (`#[cfg(test)] mod tests`) is therefore created as the very first action of Task 1, ensuring no production code precedes its tests.

Task 2 and Task 3 extend the same inline `mod tests` module with additional failing tests before implementation. This satisfies the Nyquist sampling requirement (every task has automated verification) without requiring a separate Wave 0 task — the test scaffold IS the first commit of Task 1.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| 真实 SSH 节点状态查询 | STAT-02 | 需要运行中的 DM 集群 | 配置 config.toml 含远程节点，执行 `dm-installer status` 验证表格输出 |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references (handled inline via TDD in Task 1)
- [x] No watch-mode flags
- [x] Feedback latency < 15s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved (Wave 0 inline TDD strategy validated 2026-06-15)
