---
phase: 06
slug: status
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-14
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
| 06-01-01 | 01 | 1 | STAT-01 | — | N/A | unit | `cargo test status::local` | ❌ W0 | ⬜ pending |
| 06-01-02 | 01 | 1 | STAT-02 | — | N/A | unit | `cargo test status::remote` | ❌ W0 | ⬜ pending |
| 06-01-03 | 01 | 1 | STAT-03 | — | N/A | unit | `cargo test status::table` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/status_test.rs` or `src/status/mod.rs` inline tests — stubs for STAT-01, STAT-02, STAT-03

*If none: "Existing infrastructure covers all phase requirements."*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| 真实 SSH 节点状态查询 | STAT-02 | 需要运行中的 DM 集群 | 配置 config.toml 含远程节点，执行 `dm-installer status` 验证表格输出 |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
