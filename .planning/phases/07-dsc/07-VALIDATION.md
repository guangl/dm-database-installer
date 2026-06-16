---
phase: 7
slug: dsc
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-15
---

# Phase 7 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (内置) + tracing-test |
| **Config file** | Cargo.toml [dev-dependencies] |
| **Quick run command** | `cargo test -p dm-database-installer -- cluster::dsc` |
| **Full suite command** | `cargo test -p dm-database-installer` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p dm-database-installer 2>&1 | tail -5`
- **After every plan wave:** Run `cargo test -p dm-database-installer`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 07-01-01 | 01 | 1 | DSC-01/02/03 | T-07-01 | DscStorageConfig 四字段经 validate_dsc 强制非空 | unit | `cargo test -p dm-database-installer -- config::cluster::tests` | ❌ W0 | ⬜ pending |
| 07-01-02 | 01 | 1 | DSC-01/02/03 | — | ClusterCheckpoint 6 个 DSC 字段向前兼容（#[serde(default)]） | unit | `cargo test -p dm-database-installer -- cluster::checkpoint::tests` | ❌ W0 | ⬜ pending |
| 07-02-01 | 02 | 2 | DSC-01/02/03 | — | 4 个模板函数单元测试覆盖 +DMDATA 前缀、SEQNO、GRP 结构 | unit | `cargo test -p dm-database-installer -- cluster::dsc::templates::tests` | ❌ W0 | ⬜ pending |
| 07-03-01 | 03 | 3 | DSC-01 | T-07-03 | install_only/distribute_dsc_configs MockRunner 不调用 run_dminit_remote | unit | `cargo test -p dm-database-installer --lib cluster::dsc::deploy` | ❌ W0 | ⬜ pending |
| 07-03-02 | 03 | 3 | DSC-02/03 | T-07-03 | dmasmcmd/dmasmtool 使用逻辑磁盘名 LOG0/DATA0；first_node dminit，其余节点收到 config 目录 | unit | `cargo test -p dm-database-installer --lib cluster::dsc::deploy` | ❌ W0 | ⬜ pending |
| 07-04-01 | 04 | 4 | DSC-01/02/03 | T-07-04 | 10 阶段编排 + 8 checkpoint gate 集成单元测试通过 | unit | `cargo test -p dm-database-installer -- cluster::dsc::tests` | ❌ W0 | ⬜ pending |
| 07-04-02 | 04 | 4 | DSC-01 | — | `dm-installer install dsc --config dsc.toml` 解析配置并触发 run() | manual | — | N/A | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/cluster/dsc/deploy.rs` — DSC 专有部署函数（含 MockRunner 单元测试桩）
- [ ] `src/cluster/dsc/templates.rs` — 4 个 INI 生成函数（dmdcr_cfg / dmasvrmal / dmdcr / dminit）
- [ ] `src/cluster/dsc/mod.rs` — 完整 run/run_with_runners 实现（替换占位 bail!）

*Wave 0 由 Plan 02/03/04 分别创建，Plan 01 扩展已有文件（无 Wave 0 桩需求）。*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| `dm-installer install dsc` 在真实多节点 DSC 环境完成完整部署 | DSC-01/02/03 | 需要共享块设备环境，CI 无法提供 | 参考 07-04-PLAN.md Task 2 人工验证检查单 |
| V\$INSTANCE STATUS\$=OPEN 且所有节点可查询 | DSC-01 | 同上 | 连接每个节点执行 `disql SYSDBA/... select status$ from v\$instance` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
