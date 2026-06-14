---
phase: 3
slug: cluster
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-12
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust 内建 `#[test]` + cargo-nextest |
| **Config file** | Cargo.toml（无独立 test config） |
| **Quick run command** | `cargo test` |
| **Full suite command** | `cargo nextest run` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test`
- **After every plan wave:** Run `cargo nextest run`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| ClusterConfig 反序列化 | 01 | 0 | CLUS-01 | — | N/A | unit | `cargo test config::cluster` | ❌ W0 | ⬜ pending |
| NodeConfig 字段验证 | 01 | 0 | CLUS-01 | — | 端口范围验证 | unit | `cargo test config::cluster::validate` | ❌ W0 | ⬜ pending |
| dmmal.ini 模板（主备相同） | 01 | 0 | CLUS-01 | — | N/A | unit | `cargo test cluster::templates::dmmal` | ❌ W0 | ⬜ pending |
| dmarch.ini 模板（方向相反） | 01 | 0 | CLUS-01 | — | N/A | unit | `cargo test cluster::templates::dmarch` | ❌ W0 | ⬜ pending |
| dmwatcher.ini 模板（INST_INI 差异） | 01 | 0 | CLUS-01 | — | N/A | unit | `cargo test cluster::templates::dmwatcher` | ❌ W0 | ⬜ pending |
| TCP 健康轮询超时 | 02 | 1 | CLUS-02 | — | N/A | unit | `cargo test cluster::health::timeout` | ❌ W0 | ⬜ pending |
| SSH 预检查全通过 | 02 | 1 | QUAL-01 | T-01 SSH 凭据 | 密钥优先，密码备用 | unit (mock) | `cargo test cluster::preflight::all_pass` | ❌ W0 | ⬜ pending |
| SSH 预检查单项失败报告 | 02 | 1 | QUAL-01 | — | N/A | unit (mock) | `cargo test cluster::preflight::one_fail` | ❌ W0 | ⬜ pending |
| SshError 类型覆盖 | 02 | 1 | CLUS-01 | T-02 命令注入 | 路径用 PathBuf::join | unit | `cargo test cluster::ssh` | ❌ W0 | ⬜ pending |
| CLI cluster deploy 解析 | 03 | 1 | CLUS-01 | — | N/A | unit | `cargo test cli::cluster` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/config/cluster.rs` — `ClusterConfig`/`NodeConfig`/`SshCredentials` 结构体 + 反序列化/验证单元测试
- [ ] `src/cluster/templates/mod.rs` — 所有模板函数的单元测试（dmmal/dmarch/dmwatcher）
- [ ] `src/cluster/preflight.rs` — 预检查函数接受可注入的命令执行器 trait（便于 mock）
- [ ] `src/cluster/health.rs` — `wait_tcp_ready` 超时路径单元测试
- [ ] `tests/fixtures/cluster_valid.toml` — 完整集群 TOML 示例（集成测试基准）
- [ ] `tests/fixtures/cluster_invalid_no_primary.toml` — 无 primary 节点时验证失败

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| 端到端 SSH 集群部署 | CLUS-01, CLUS-02, QUAL-01 | 需要真实 Linux 节点 + 达梦安装包 | 用 Docker sshd 容器模拟两节点，或真实 VM；执行 `dm-installer cluster deploy --config cluster_valid.toml` 观察日志顺序 |
| disql CLI 批量 SQL 参数格式 | CLUS-01 (SQL 设置步骤) | disql 参数格式 [ASSUMED]，需实机验证 | 在安装达梦的节点上执行 `disql SYSDBA/SYSDBA@localhost:5236 -e "select status from v$instance;"` 验证参数格式 |
| 主节点 TCP 就绪前备节点不启动 | CLUS-02 | 时序依赖，难以纯单元测试 | 观察 tracing 日志：`[node:primary]` 健康确认条目必须在 `[node:standby]` 启动条目之前 |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
