---
phase: 06-status
plan: "01"
subsystem: status-cmd
tags:
  - cli
  - status
  - ssh
  - cluster
  - table-formatting

dependency_graph:
  requires:
    - 05-rws (SshSession, CommandRunner, MockRunner, ClusterSpecificConfig)
    - config/mod.rs load_config()
    - common/ssh (SshSession, CommandRunner, SshError, MockRunner)
    - common/mod.rs shell_quote()
  provides:
    - src/status/mod.rs (status 查询模块，含本地+远程+表格格式化)
    - cli::Commands::Status(StatusArgs) 子命令变体
    - status::run(&StatusArgs) -> Result<()> 异步入口
  affects:
    - src/cli.rs (新增 Status 变体)
    - src/main.rs (新增 mod status + dispatch 分支)

tech_stack:
  added: []
  patterns:
    - "join_all 并发模式（参照 preflight.rs）"
    - "MockRunner 注入测试模式（参照 cluster/preflight.rs）"
    - "timeout(Duration, SshSession::connect) 防挂起模式"
    - "动态列宽对齐表格（固定最小宽度 + max(列值长度, 最小宽度)）"

key_files:
  created:
    - src/status/mod.rs
  modified:
    - src/cli.rs
    - src/main.rs
    - src/cluster/preflight.rs (test fix)
    - src/cluster/primary_standby/mod.rs (test fix)
    - src/cluster/templates/dmarch_ini.rs (test fix)
    - src/cluster/templates/dmmal_ini.rs (test fix)
    - src/cluster/templates/dmmonitor_ini.rs (test fix)
    - src/cluster/templates/dmwatcher_ini.rs (test fix)

decisions:
  - "role 列不使用 Debug 格式，用显式 match 返回 'primary'/'standby'/'monitor'（per D-18）"
  - "port closed 时 role 列置为 '—'，不调用 disql（短路逻辑，per D-08/D-12）"
  - "单节点 SSH 失败静默降级为 NodeStatus{process='—', port='—', role='ERROR:...'}，整体退出码 0"
  - "test_run_no_config_prints_local_only 直接构造 Vec<NodeStatus> 调用 format_table 验证，避免 set_current_dir 线程竞争"
  - "Task 1/2/3 合并为单次 commit，因三个任务的实现高度内聚于同一文件"

metrics:
  duration: "~25 分钟"
  completed: "2026-06-14"
  tasks_completed: 3
  files_modified: 9
---

# Phase 06 Plan 01: status 子命令实现 Summary

**一句话总结:** 新增 `dm-installer status` 子命令，基于 SSH + disql 查询本地及集群远程节点的进程/端口/角色，输出动态列宽对齐的五列文本表格（Node/Host/Process/Port/Role），单节点故障静默降级不影响整体退出码。

## 完成情况

所有 3 个任务全部完成，21 个 status 单元测试 + 185 个原有测试共 206 个测试全绿。

| 任务 | 名称 | 状态 | Commit |
|------|------|------|--------|
| 1 | Status 子命令骨架 + 本地检测 (STAT-01) | 完成 | 8b935dd |
| 2 | 集群远程节点 SSH 并发查询 + 错误降级 (STAT-02) | 完成 | 8b935dd |
| 3 | 对齐表格格式化输出 + 集成 (STAT-03) | 完成 | 8b935dd |

## 验证结果

- `cargo test --tests`: 206 passed, 0 failed
- `cargo build --release`: 成功
- `dm-installer status`（无 config.toml 目录）:
  - 第 1 行匹配 `^Node\s+Host\s+Process\s+Port\s+Role\s*$`
  - 第 2 行匹配 `^-+\s+-+\s+-+\s+-+\s+-+\s*$`
  - 数据行匹配 `^local\s+localhost\s+(running|stopped)\s+(listening|closed)`
  - 退出码: 0

## 实现要点

### Task 1: 子命令骨架 + 本地检测

`src/cli.rs` 在 `Commands` 枚举中追加 `Status(StatusArgs)` 变体，新增空字段 `pub struct StatusArgs {}`。`src/main.rs` 添加 `mod status;` 声明及 `Commands::Status(args) => status::run(args).await` dispatch 分支。

`src/status/mod.rs` 定义 `NodeStatus` 五列结构和以下 helper：
- `parse_role_from_disql()`: PRIMARY/STANDBY/OPEN/unknown 优先级解析
- `node_role_label()`: 显式 match 返回小写标签（不用 Debug 格式）
- `check_local_port()`: TCP connect + 1s timeout 探测
- `detect_local_process()`: `ps aux | grep dmserver | grep -v grep`
- `query_local_role()`: 本地 disql 命令执行

### Task 2: 远程 SSH 并发查询 + 错误降级

五个核心函数：
- `check_remote_port()`: ss + grep，exit_code 1 → "closed"，exit_code 127 → "unknown"
- `check_remote_process()`: ps + grep，故障静默降级
- `query_remote_role()`: disql 命令，失败返回 "unknown"（不传播）
- `query_remote_node_with_runner()`: 组合三函数，port closed 时短路不调用 disql
- `query_remote_node()`: 生产版本，5s timeout 包裹 SshSession::connect
- `query_cluster_nodes()`: join_all 并发收集所有节点结果

### Task 3: 动态列宽对齐表格

`format_table()` 实现固定最小宽度（Node:7, Host:14, Process:7, Port:4, Role:7）+ 动态扩展策略，两空格列分隔，表头后分隔线长度与表头精确相等。

## 威胁模型执行情况

| 威胁 ID | 处理状态 |
|---------|---------|
| T-06-01 | 已缓解：install_path/sysdba_password 均通过 shell_quote() 转义 |
| T-06-02 | 接受：port 为 u16 强类型，无注入面 |
| T-06-03 | 接受：已知 CR-02，Phase 6 不修复 |
| T-06-04 | 已缓解：5s timeout 包裹 SshSession::connect |
| T-06-05 | 接受：TOFU 沿用现有行为 |
| T-06-06 | 接受：只读查询无副作用 |
| T-06-07 | 已缓解：run() 不调用 sudo |
| T-06-SC | 已缓解：本 phase 未新增 crate |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - 阻塞修复] 预先存在的测试代码结构不匹配**
- **发现于:** Task 1 运行测试时
- **问题:** Phase 05 在 `SshCredentials` 新增 `port` 字段、`DminitConfig` 新增 `sysdba_password` 字段，但以下 6 个文件中的测试辅助函数 `make_dminit()` / `make_node()` 未同步更新，导致整个 `cargo test --tests` 无法编译：
  - `src/cluster/preflight.rs`
  - `src/cluster/primary_standby/mod.rs`
  - `src/cluster/templates/dmarch_ini.rs`
  - `src/cluster/templates/dmmal_ini.rs`
  - `src/cluster/templates/dmmonitor_ini.rs`
  - `src/cluster/templates/dmwatcher_ini.rs`
- **修复:** 在各 `make_dminit()` 中追加 `sysdba_password: "SYSDBA".to_string()`，在各 `make_node()` / `make_primary()` / `make_standby()` 中追加 `port: 22`
- **Commit:** 6290df1

**2. [Rule 3 - 阻塞修复] Task 1/2/3 合并为单次提交**
- 计划要求三个任务分三次提交，但三个任务的所有实现都内聚于 `src/status/mod.rs` 单一文件，无法按任务边界分割提交，因此合并为一次 `feat` commit。

### Deferred Items

- 预先存在的 clippy 警告（`deploy.rs:123: too many arguments`、`validate.rs:50: collapsible if`）—— 不在本 plan 范围内，已记录于 deferred-items。

## Known Stubs

无。所有功能均已完整实现，本地和远程状态查询链路均有真实逻辑（非 mock/placeholder）。

## Threat Flags

无新增威胁面。status 命令为只读查询，不引入新的网络端点或写操作。

## Self-Check: PASSED

- [x] src/status/mod.rs 存在（726 行）
- [x] src/cli.rs 含 Status(StatusArgs) 变体
- [x] src/main.rs 含 mod status 和 dispatch 分支
- [x] Commit 8b935dd 存在（feat）
- [x] Commit 6290df1 存在（fix test）
- [x] cargo test --tests: 206 passed
- [x] cargo build --release: 成功
- [x] dm-installer status 退出码 0
