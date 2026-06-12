---
phase: 2
slug: toml
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-12
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in) |
| **Config file** | `Cargo.toml [dev-dependencies]` |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo test -- --include-ignored` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo test -- --include-ignored`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** ~5 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-01-01 | 01 | 0 | INST-02 SC3 | — | `validate_install_config` 拒绝无效枚举值，错误含字段名 | unit | `cargo test config::validate_install_config` | ❌ W0 | ⬜ pending |
| 02-01-02 | 01 | 0 | INST-02 SC3 | — | port=0 被拒绝，错误含"port" | unit | `cargo test config::test_port_zero` | ❌ W0 | ⬜ pending |
| 02-01-03 | 01 | 0 | INST-02 | — | `load_and_validate` 读取 tempfile fixture 成功返回 InstallConfig | unit | `cargo test config::test_load_and_validate` | ❌ W0 | ⬜ pending |
| 02-01-04 | 01 | 0 | QUAL-03 | — | `validate::run` 语义验证覆盖（page_size=12 被拒绝） | unit | `cargo test config::validate::test_semantic_invalid` | ❌ W0 | ⬜ pending |
| 02-02-01 | 02 | 1 | INST-02 SC4 | — | `install --config` 分支使用 config 文件值而非默认值 | unit | `cargo test install::test_config_branch` | ❌ W0 | ⬜ pending |
| 02-02-02 | 02 | 1 | INST-02 | — | dminit 命令含 config 文件指定的 port/page_size/charset | unit | `cargo test install::init::` | ✅ 已有 | ⬜ pending |
| 02-02-03 | 02 | 1 | INST-02 SC3 | T-XML-INJ | config 路径字段经 xml_escape 转义后传入 dminit XML | unit | `cargo test install::silent_install::` | ✅ 已有 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/config/mod.rs` — `validate_install_config()` 单元测试：page_size/charset/extent_size 无效值被拒绝，所有有效值通过，port=0 被拒绝
- [ ] `src/config/mod.rs` — `load_and_validate()` 集成测试：使用 tempfile fixture 验证三步链（读文件 → TOML 解析 → 语义验证）
- [ ] `src/install/mod.rs` — `--config` 条件分支测试：config 文件中的 port/page_size 值被 dminit 命令使用
- [ ] `tests/fixtures/semantic_invalid.toml` — `page_size = 12` 的语义非法 fixture（区别于现有语法错误 fixture）

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| `dm-installer install --config valid.toml --package /path/to/dm.iso` 完整安装成功 | INST-02 SC1 | 需要真实达梦安装包和 root 权限环境 | 在 Linux x86_64 VM 上运行完整安装命令，验证参数生效 |
| 参数确认 UI 显示 config 文件中的值 | INST-02 SC2 | 需要 TTY 环境交互 | 不带 `--yes` 运行 `install --config`，观察 PAGE_SIZE 等显示值是否与 TOML 文件一致 |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 5s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
