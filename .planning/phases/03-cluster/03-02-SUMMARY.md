---
phase: 03-cluster
plan: "02"
subsystem: cluster-ssh-infrastructure
tags: [cluster, ssh, russh, sftp, preflight, health, tcp, mock]
dependency_graph:
  requires: [03-01]
  provides: [SshError, CommandRunner, SshSession, TofuHandler, MockRunner, check_sudo_nopass, check_port_available, check_disk_space, check_node, preflight_all_nodes, wait_tcp_ready]
  affects: [src/cluster/mod.rs, Cargo.toml]
tech_stack:
  added: [russh 0.61.2 (ring backend), russh-sftp 2.3.0, async-trait 0.1, futures 0.3]
  patterns: [tofu-host-key, command-runner-trait-injection, join_all-concurrent, tokio-timeout-polling]
key_files:
  created:
    - src/cluster/ssh.rs
    - src/cluster/preflight.rs
    - src/cluster/health.rs
  modified:
    - Cargo.toml
    - src/cluster/mod.rs
decisions:
  - "russh features: async-trait + ring (不用 aws-lc-rs)，ring 是 russh 0.61.2 的稳定 crypto backend，无 OpenSSL C 依赖"
  - "async-trait 作为独立依赖引入（Cargo.toml async-trait = 0.1），russh async-trait feature 仅启用 russh 内部 trait 的 async-trait 支持，不重导出 macro"
  - "check_port_available 将 grep exit_code=1（无匹配）视为端口空闲 Ok，避免误报"
  - "MockRunner 使用前缀匹配 starts_with 而非精确匹配，允许 preflight 的 ss/df 命令含动态参数时仍能匹配"
  - "preflight_all_nodes 使用 Arc<dyn CommandRunner> 而非 Box<dyn>，允许 Plan 03 在 preflight/安装/启动多步复用同一 runner 实例"
metrics:
  duration_minutes: 7
  completed_date: "2026-06-13T00:16:00Z"
  tasks_completed: 5
  tasks_total: 5
  files_created: 3
  files_modified: 2
  tests_added: 11
---

# Phase 03 Plan 02: SSH 远程操作基础设施 Summary

**一行概述:** 以 russh 0.61.2 (ring backend，无 OpenSSL C 依赖) 实现 SshError + CommandRunner trait + TofuHandler + 真实 SshSession + MockRunner，并在此基础上完成 QUAL-01 三项预检查（sudo/端口/磁盘）和 CLUS-02 的 TCP 健康轮询原语。

## Tasks Completed

| Task | Name | Commit | Key Files |
|------|------|--------|-----------|
| 1 | russh/russh-sftp 包合法性 checkpoint（auto-approved） | — | — |
| 2 | 引入 russh 0.61.2 + russh-sftp 2.3.0 + futures 0.3 | 8b99df8 | Cargo.toml |
| 3 | SshError + CommandRunner + SshSession + TofuHandler + MockRunner | 727d7d0 | src/cluster/ssh.rs |
| 4 | preflight 三项预检查 + preflight_all_nodes | f6f3db7 | src/cluster/preflight.rs |
| 5 | wait_tcp_ready TCP 健康轮询 | 0da0dca | src/cluster/health.rs |

## Cargo.toml 实际新增依赖

```toml
russh = { version = "0.61.2", default-features = false, features = ["async-trait", "ring"] }
russh-sftp = "2.3.0"
futures = "0.3"
async-trait = "0.1"
```

**注：** `client` feature 在 russh 0.61.2 中不存在（偏差 Rule 1 自动修复）；需显式添加 `ring` crypto backend（编译错误提示修复）；`async_trait` macro 需从独立 `async-trait` crate 引入（russh async-trait feature 仅内部使用不重导出）。

## SshError 三个 variant 最终错误消息格式

| Variant | 错误消息模板 |
|---------|------------|
| `Connect { host, source }` | `SSH 连接失败 {host}: {source}` |
| `ExecFailed { command, exit_code }` | `SSH 命令执行失败 (exit {exit_code}): {command}` |
| `SftpUpload { remote_path, source }` | `SFTP 上传失败 {remote_path}: {source}` |

## CommandRunner trait 方法签名（供 Plan 03 调用）

```rust
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn exec(&self, command: &str) -> Result<(Vec<u8>, u32), SshError>;
    async fn sftp_write(&self, remote_path: &str, bytes: &[u8]) -> Result<(), SshError>;
}
```

## MockRunner 构造与使用示例（供 Plan 03 集成测试参考）

```rust
use std::sync::Arc;
use crate::cluster::ssh::{CommandRunner, MockRunner};

// 构造：Vec<(command_prefix, exit_code, stdout_bytes)>
let runner = MockRunner::new(vec![
    ("sudo -n true".to_string(), 0, vec![]),
    ("ss -tlnp | grep ':5236'".to_string(), 1, vec![]),
    ("df -B1 /opt".to_string(), 0, b"...".to_vec()),
]);

// 单独使用（preflight/health 注入）
let result = runner.exec("sudo -n true").await;

// Arc 包装用于 preflight_all_nodes（Plan 03 多步复用）
let arc_runner: Arc<dyn CommandRunner> = Arc::new(runner);
preflight_all_nodes(vec![(node, arc_runner)]).await?;
```

**注：** MockRunner 使用命令前缀匹配（`starts_with`），匹配后从列表移除（有序消费）。`sftp_write` 记录到 `sftp_writes` 字段，可用于断言上传调用。

## wait_tcp_ready 实测耗时

| 测试 | max_secs | 实测耗时 | 结果 |
|------|---------|---------|------|
| Test 1 (超时路径) | 2s | ~2.00s | Err "127.0.0.1:1 在 2s 内未就绪" |
| Test 2 (立即就绪) | 5s | < 10ms | Ok |

## Verification Results

```
cargo build          — OK (warnings only, expected for stub phase)
cargo tree | grep openssl/native-tls — 返回非零（依赖树无 C FFI）
cargo test cluster::ssh      — 4 passed
cargo test cluster::preflight — 5 passed
cargo test cluster::health   — 2 passed
cargo test (全库)           — 67 passed; 0 failed
```

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] russh `client` feature 不存在**
- **Found during:** Task 2
- **Issue:** `cargo add` 尝试 `features = ["async-trait", "client"]` 时，cargo 报告 `russh v0.61.2` 不存在 `client` feature，只有 `_bench, async-trait, aws-lc-rs, default, des, dsa, flate2, legacy-ed25519-pkcs8-parser, ring, rsa, serde, yasna`
- **Fix:** 移除 `client` feature，改为 `features = ["async-trait", "ring"]`（russh 所有代码在 default-features=false 时仍完整可用）
- **Files modified:** Cargo.toml
- **Commit:** 8b99df8

**2. [Rule 3 - Blocking] russh 需要显式 crypto backend**
- **Found during:** Task 2
- **Issue:** `cargo build` 报 `russh requires enabling either ring or aws-lc-rs feature as a crypto backend`
- **Fix:** 添加 `ring` feature，与 CLAUDE.md 的 rustls/ring 策略一致（避免 OpenSSL）
- **Files modified:** Cargo.toml
- **Commit:** 8b99df8

**3. [Rule 3 - Blocking] `ssh_key` 未解析为 crate**
- **Found during:** Task 3
- **Issue:** `TofuHandler` 中使用 `ssh_key::PublicKey` 时编译器报 `cannot find module or crate ssh_key`
- **Fix:** 改为 `russh::keys::PublicKey`（russh 重导出 ssh_key::PublicKey，路径为 `russh::keys::PublicKey`）
- **Files modified:** src/cluster/ssh.rs
- **Commit:** 727d7d0

**4. [Rule 3 - Blocking] `async_trait` macro 未导出**
- **Found during:** Task 3
- **Issue:** `use async_trait::async_trait` 无法解析——russh 的 `async-trait` feature 仅内部启用，不重导出 macro
- **Fix:** 在 Cargo.toml 添加独立依赖 `async-trait = "0.1"`
- **Files modified:** Cargo.toml
- **Commit:** 727d7d0

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: information_disclosure | src/cluster/ssh.rs | try_auth 函数日志路径不含凭据（T-03-05 已 mitigate）；SshCredentials.password 已在 Plan 01 加 #[serde(skip_serializing)] |
| threat_flag: tofu_host_key | src/cluster/ssh.rs | TofuHandler 无条件接受任何服务器密钥（D-07 MVP 策略，T-03-04 accepted，已在代码注释中说明） |

## Known Stubs

无。本 Plan 实现的所有函数均有完整逻辑，无占位返回值。

## Self-Check: PASSED

- [x] src/cluster/ssh.rs — 存在
- [x] src/cluster/preflight.rs — 存在
- [x] src/cluster/health.rs — 存在
- [x] Cargo.toml 含 russh — 存在
- [x] commit 8b99df8 (Task 2 deps) — 存在
- [x] commit 727d7d0 (Task 3 ssh.rs) — 存在
- [x] commit f6f3db7 (Task 4 preflight.rs) — 存在
- [x] commit 0da0dca (Task 5 health.rs) — 存在
- [x] 67 tests all pass — 验证
