---
phase: 4
slug: release
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-13
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (unit) + cargo build --target (cross-compile smoke) |
| **Config file** | Cargo.toml |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo test && cargo build --release` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo test && cargo build --release`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 4-01-01 | 01 | 0 | — | — | sftp CREATE flag 修复（不截断已有文件） | integration | `cargo test` | ✅ | ⬜ pending |
| 4-01-02 | 01 | 0 | — | — | ISO 提取链路完整（无 panic） | unit | `cargo test` | ✅ | ⬜ pending |
| 4-01-03 | 01 | 0 | — | T-3-04 | shell 注入修复（参数逃逸） | unit | `cargo test test_shell_escape` | ❌ W0 | ⬜ pending |
| 4-02-01 | 02 | 1 | PLAT-01 | — | cargo-dist 配置正确（dist plan 输出含三平台） | manual | `cargo dist plan` | ❌ W0 | ⬜ pending |
| 4-02-02 | 02 | 1 | PLAT-04 | — | install windows 子命令可解析 TOML，路径不 panic | unit | `cargo test test_windows_placeholder` | ❌ W0 | ⬜ pending |
| 4-03-01 | 03 | 2 | PLAT-01 | — | Linux x86_64 二进制能在目标机运行 | manual | CI: `cargo build --target x86_64-unknown-linux-gnu` | ❌ W0 | ⬜ pending |
| 4-03-02 | 03 | 2 | PLAT-02 | — | Linux aarch64 二进制能在 ARM 机器运行 | manual | CI: `cargo build --target aarch64-unknown-linux-gnu` | ❌ W0 | ⬜ pending |
| 4-03-03 | 03 | 2 | PLAT-03 | — | Windows 二进制能在 Windows 上启动 | manual | CI: Windows runner build | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/test_sftp_flags.rs` 或在现有 tests 中添加 — sftp CREATE flag 回归测试
- [ ] `tests/test_shell_escape.rs` 或在现有 tests 中添加 — shell 参数注入防护测试
- [ ] `tests/test_windows_placeholder.rs` — PLAT-04 Windows 子命令解析不 panic

*注：cargo-dist 配置和三平台 CI 构建验证主要是 Wave 1/2 的 CI 集成测试，不依赖 Wave 0 测试文件。*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Linux x86_64 curl\|sh 下载安装器 | PLAT-01 | 需要公开 GitHub Release URL 和真实网络 | 创建 tag → Release 发布后，在干净 Linux x86_64 机器运行 `curl -sSf <url> \| sh` |
| Linux aarch64 curl\|sh 下载安装器 | PLAT-02 | 需要 ARM 机器 | 同上，在 aarch64 机器验证 |
| Windows SSH 到 Linux 节点安装 | PLAT-03 | 需要 Windows 机器 + Linux target | 在 Windows 运行 `dm-installer install --config config.toml` 含远程节点配置 |
| GitHub Actions tag 触发 Release | PLAT-01~03 | 需要真实 CI 运行 | push `v0.1.0` tag 后观察 Actions 是否触发并生成 Release assets |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
