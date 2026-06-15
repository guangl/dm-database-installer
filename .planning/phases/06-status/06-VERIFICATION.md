---
phase: 06-status
verified: 2026-06-15T10:00:00Z
status: passed
score: 6/6
overrides_applied: 0
re_verification: false
---

# Phase 6: status 命令 — 验证报告

**Phase Goal:** `dm-installer status` 命令查询本地及所有远程节点的进程/端口/角色状态，输出对齐表格
**Verified:** 2026-06-15T10:00:00Z
**Status:** PASSED
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | 用户执行 `dm-installer status` 后在终端看到本地 DM 实例的进程状态与端口监听 | VERIFIED | 运行输出：`local    localhost       stopped  closed  —`，退出码 0；local 数据行格式匹配 `^local\s+localhost\s+(running\|stopped)\s+(listening\|closed)` |
| 2 | 若当前目录存在 config.toml，命令自动通过 SSH 查询配置中所有远程节点状态 | VERIFIED | `run()` 中 `config::load_config().ok()` → Cluster 分支调用 `query_cluster_nodes(&specific).await`，join_all 并发 SSH 查询，结果追加到 rows；test_query_cluster_nodes_concurrent_collects_all 覆盖此路径 |
| 3 | 输出表格包含每节点的进程状态（running/stopped）、端口是否监听、数据库角色（PRIMARY/STANDBY/OPEN） | VERIFIED | `format_table()` 输出五列（Node/Host/Process/Port/Role）对齐表格；实际运行确认表头行匹配 `^Node\s+Host\s+Process\s+Port\s+Role\s*$`；parse_role_from_disql 正确解析 PRIMARY/STANDBY/OPEN/unknown |
| 4 | 某节点 SSH 连接失败时，该行显示错误原因，其余节点正常输出，退出码 0 | VERIFIED | `query_remote_node()` 中 timeout Err → `NodeStatus{process="—", port="—", role="ERROR: 连接超时"}`；Ok(Err(e)) → `role: format!("ERROR: {}", e)`；join_all 收集所有节点结果不因单节点失败中断；test_format_table_error_row 验证错误行格式；run() 返回 Ok(()) |
| 5 | cargo test 全套 206 个测试全绿，含 22 个 status 单元测试 | VERIFIED | 实际运行 `cargo test`：`test result: ok. 206 passed; 0 failed` |
| 6 | cargo build --release 成功 | VERIFIED | 实际运行 `cargo build --release`：`Finished release profile` 无 error |

**Score:** 6/6 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/cli.rs` | `Status(StatusArgs)` 枚举变体 + `StatusArgs` 空结构 | VERIFIED | Line 31: `Status(StatusArgs),`；Lines 77-79: `pub struct StatusArgs {}` |
| `src/main.rs` | `mod status;` 声明 + `Commands::Status` dispatch 分支 | VERIFIED | Line 12: `mod status;`；Line 37: `cli::Commands::Status(args) => status::run(args).await,` |
| `src/status/mod.rs` | 完整 status 模块，min 200 行，含所有函数和测试 | VERIFIED | 711 行；包含 13 个函数（parse_role_from_disql, node_role_label, check_local_port, detect_local_process, query_local_role, check_remote_port, check_remote_process, query_remote_role, query_remote_node_with_runner, query_remote_node, query_cluster_nodes, format_table, run）+ 22 个测试 |
| `Cargo.toml` | `futures = "0.3"` 已在 dependencies | VERIFIED | `futures = "0.3"` 已存在于 `[dependencies]` |

### Key Link Verification

| From | To | Via | Status | Details |
|------|-----|-----|--------|---------|
| `src/main.rs` | `src/status/mod.rs` | `Commands::Status(args) => status::run(args).await` | WIRED | Line 37 of main.rs 精确匹配；`mod status;` Line 12 声明 |
| `src/status/mod.rs` | `src/common/ssh/session.rs` | `SshSession::connect(host, 22, &node.ssh)` | WIRED | Line 164: `SshSession::connect(&node.host, 22, &node.ssh)` |
| `src/status/mod.rs` | `src/common/ssh/runner.rs` | `runner.exec(&cmd).await` | WIRED | Lines 93, 104, 125 均调用 `runner.exec(&cmd).await` |
| `src/status/mod.rs` | `src/config/mod.rs` | `config::load_config().ok()` 失败降级为 None | WIRED | Line 274: `let cfg = config::load_config().ok();` |
| `src/status/mod.rs` | `src/common/mod.rs` | `crate::common::shell_quote()` 转义 disql 命令 | WIRED | Line 6: `use crate::common::shell_quote;`；Lines 73, 74, 121, 122 调用 |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|-------------------|--------|
| `src/status/mod.rs` run() | `process_str` | `detect_local_process()` → `ps aux \| grep dmserver \| grep -v grep` | 真实 shell 命令输出 | FLOWING |
| `src/status/mod.rs` run() | `port_str` | `check_local_port(port)` → `TcpStream::connect` + 1s timeout | 真实 TCP 探测 | FLOWING |
| `src/status/mod.rs` run() | `role` | `query_local_role()` → 本地 disql 命令（仅 port listening 时） | 真实 disql 输出或短路 "—" | FLOWING |
| `src/status/mod.rs` run() | `remote_rows` | `query_cluster_nodes()` → join_all SSH 并发查询 | 真实 SSH 连接 + 远程命令 | FLOWING |
| `src/status/mod.rs` run() | `table` (print) | `format_table(&rows)` 动态列宽格式化 | 真实数据行，非硬编码 | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| 表头格式匹配 5 列正则 | `dm-installer status \| head -1 \| grep -E '^Node\s+Host\s+Process\s+Port\s+Role\s*$'` | HEADER_MATCHES | PASS |
| local 数据行格式正确 | `dm-installer status \| grep -E '^local\s+localhost\s+(running\|stopped)\s+(listening\|closed)' \| wc -l` | 1 | PASS |
| 退出码 0 | `dm-installer status; echo $?` | 0 | PASS |
| 全套测试通过 | `cargo test` | 206 passed, 0 failed | PASS |
| release 编译成功 | `cargo build --release` | Finished 无 error | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|---------|
| STAT-01 | 06-01-PLAN.md | 用户可执行 `dm-installer status` 查询本地 DM 实例进程状态与端口监听 | SATISFIED | detect_local_process() + check_local_port() + local 行输出；test_check_local_port_closed 等 7 个 Task 1 测试 |
| STAT-02 | 06-01-PLAN.md | status 命令读取 config.toml 节点列表，通过 SSH 查询所有远程节点状态 | SATISFIED | query_cluster_nodes() → join_all → query_remote_node() → SshSession::connect；9 个 Task 2 测试覆盖所有降级路径 |
| STAT-03 | 06-01-PLAN.md | 状态输出包含进程状态、端口监听、数据库角色，格式为对齐表格 | SATISFIED | format_table() 动态列宽五列表格；parse_role_from_disql() PRIMARY/STANDBY/OPEN/unknown；6 个 Task 3 测试覆盖；实际运行确认格式 |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `src/cluster/deploy.rs` | 123 | clippy::too-many-arguments（8/7 参数）| INFO | 预先存在（Phase 5 引入），Phase 6 未修改此文件；不阻塞目标 |
| `src/config/validate.rs` | 50 | clippy::collapsible-if | INFO | 预先存在（Phase 5 引入），Phase 6 未修改此文件；不阻塞目标 |

> **说明：** 上述 2 个 clippy 警告均位于 Phase 6 未修改的文件（`src/cluster/deploy.rs`、`src/config/validate.rs`），由 Phase 5 提交引入，在 SUMMARY 中已明确标注为 deferred items。Phase 6 修改的三个文件（`src/cli.rs`、`src/main.rs`、`src/status/mod.rs`）无新增 clippy 警告、无 TBD/FIXME/XXX 标记。

### Human Verification Required

无需人工验证。所有可观测真值均已通过程序化验证确认。

### Gaps Summary

无 gaps。Phase 6 目标完整实现：

- STAT-01: 本地进程/端口/角色检测，22 个测试覆盖
- STAT-02: 远程 SSH 并发查询 + 5 种失败降级路径（连接超时、连接拒绝、ss 缺失/exit 127、grep 无匹配/exit 1、disql 失败）
- STAT-03: 动态列宽对齐表格，五列（Node/Host/Process/Port/Role），分隔线长度精确匹配表头
- 实际运行 `dm-installer status` 在无 config.toml 目录输出格式正确的表格，退出码 0

---

_Verified: 2026-06-15T10:00:00Z_
_Verifier: Claude (gsd-verifier)_
