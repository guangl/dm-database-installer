---
phase: 1
slug: curl-sh
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-12
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (built-in) + `cargo-nextest` (推荐) |
| **Config file** | none — Wave 0 无需独立配置 |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo test -- --include-ignored` |
| **Estimated runtime** | ~5 seconds (unit tests only) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo test -- --include-ignored`
- **Before `/gsd:verify-work`:** Full suite must be green + manual e2e on real Linux + DM ISO
- **Max feedback latency:** ~5 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 01-T01 | Cargo deps | 1 | INST-01 | — | N/A | unit | `cargo build` | ❌ W0 | ⬜ pending |
| 01-T02 | CLI structure | 1 | INST-01 | — | clap validates --package path | unit | `cargo test cli::tests::test_install_args` | ❌ W0 | ⬜ pending |
| 01-T03 | Idempotency check | 1 | QUAL-02 | — | dm.ini exists → exit 0 | unit | `cargo test install::idempotent::tests::test_existing_instance` | ❌ W0 | ⬜ pending |
| 01-T04 | SHA-256 verify | 1 | DOWN-02 | T-path-traversal | correct hash passes, wrong hash fails | unit | `cargo test install::checksum::tests::test_sha256` | ❌ W0 | ⬜ pending |
| 01-T05 | Param confirm | 2 | INST-03 | — | --defaults skips stdin read | unit | `cargo test install::tests::test_confirm_params` | ❌ W0 | ⬜ pending |
| 01-T06 | XML generation | 2 | INST-01 | T-xml-injection | XML special chars escaped | unit | `cargo test install::silent_install::tests::test_xml_generation` | ❌ W0 | ⬜ pending |
| 01-T07 | dminit call | 2 | INST-01 | T-path-traversal | no spaces in key=value args | unit | `cargo test install::init::tests::test_dminit_cmd` | ❌ W0 | ⬜ pending |
| 01-T08 | Service register | 2 | INST-04 | — | correct service name DmServiceDMSERVER | unit | `cargo test install::service::tests::test_service_cmd` | ❌ W0 | ⬜ pending |
| 01-T09 | validate cmd | 1 | QUAL-03 | — | valid TOML → Ok, invalid → Err | unit | `cargo test config::validate::tests::test_validate_config` | ❌ W0 | ⬜ pending |
| 01-T10 | download skeleton | 1 | DOWN-01 | — | returns descriptive error, not panic | unit | `cargo test download::tests::test_fetch_stub` | ❌ W0 | ⬜ pending |
| 01-T11 | e2e install | 3 | INST-01 | — | instance starts, accepts connections | manual-only | — | N/A | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/cli.rs` — CLI 结构体定义，覆盖 INST-01（ClI 解析）
- [ ] `src/install/checksum.rs` + `tests/` — SHA-256 单元测试，覆盖 DOWN-02
- [ ] `src/install/idempotent.rs` + `tests/` — 幂等性检测单元测试，覆盖 QUAL-02
- [ ] `src/config/validate.rs` + `tests/fixtures/valid.toml` + `tests/fixtures/invalid.toml` — validate 子命令，覆盖 QUAL-03
- [ ] `src/install/service.rs` + `tests/` — 服务注册命令构建，覆盖 INST-04
- [ ] `src/install/silent_install.rs` + `tests/` — XML 生成，覆盖 INST-01 XML injection threat
- [ ] `src/install/init.rs` + `tests/` — dminit 命令构建，覆盖 INST-01 dminit pitfall
- [ ] `src/download/mod.rs` + `tests/` — 下载骨架返回正确错误，覆盖 DOWN-01 placeholder

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| 完整安装流程在真实 Linux 上运行 | INST-01 (e2e) | 需要真实 DM ISO 和 Linux x86_64 环境 | 1) 在 Linux x86_64 VM 运行 `sudo dm-installer install --package dm8.iso --defaults`; 2) 验证 `systemctl status DmServiceDMSERVER.service` 为 active; 3) 连接 5236 端口确认实例响应 |
| systemctl 服务注册和开机自启 | INST-04 | 需要 systemd 环境 | `systemctl is-enabled DmServiceDMSERVER.service` 应输出 `enabled` |
| dminit 等号两侧无空格 | INST-01 | 需运行真实 dminit | 安装后 `dm.ini` 中 PAGE_SIZE/PORT_NUM 与命令行参数一致 |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
