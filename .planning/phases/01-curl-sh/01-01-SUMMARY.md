---
phase: 01-curl-sh
plan: "01"
subsystem: cli-skeleton
tags:
  - rust
  - clap
  - walking-skeleton
  - tokio
dependency_graph:
  requires: []
  provides:
    - cli::Cli
    - cli::Commands
    - cli::InstallArgs
    - cli::ValidateArgs
    - config::InstallConfig
    - install::run (placeholder)
    - download::fetch_dm_installer (placeholder)
    - ui::print_status
    - ui::confirm_immutable_params (placeholder)
  affects: []
tech_stack:
  added:
    - clap 4.6.1 (derive)
    - tokio 1.52.3 (full)
    - serde 1.0.228 (derive)
    - toml 1.1.2
    - anyhow 1.0.102
    - thiserror 2.0.18
    - sha2 0.11.0
    - indicatif 0.18.4
    - console 0.16.3
    - tracing 0.1.44
    - tracing-subscriber 0.3.23 (env-filter)
    - tempfile 3.27.0
    - reqwest 0.13.4 (rustls + stream, no native-tls)
    - clap_complete 4.6.5
  patterns:
    - clap derive macro (Cli/Commands/Args structs)
    - tokio::main async entry point
    - tracing + EnvFilter for structured logging
    - anyhow::Result chain with context()
key_files:
  created:
    - Cargo.toml
    - Cargo.lock
    - src/main.rs
    - src/cli.rs
    - src/ui.rs
    - src/install/mod.rs
    - src/download/mod.rs
    - src/config/mod.rs
    - src/config/validate.rs
  modified: []
decisions:
  - "reqwest 0.13.4 的 TLS feature 名为 rustls（非 rustls-tls），已修正 Cargo.toml"
  - "config/validate.rs 在 Plan 01 已实现完整 TOML 解析（超出骨架最小需求，提前交付 QUAL-03 基础）"
  - "InstallConfig 已含完整 8 个字段（Phase 2 扩展点直接可用）"
metrics:
  duration: "4 minutes"
  completed_date: "2026-06-12"
  tasks_completed: 2
  files_created: 9
---

# Phase 1 Plan 01: Walking Skeleton — CLI 骨架与依赖锁定

**一句话总结：** clap 4.6.1 derive 宏三子命令（install/validate/completions）+ tokio main + tracing EnvFilter + 全模块占位骨架，14 个依赖版本锁定，无 native-tls 依赖。

## What Was Built

建立了 dm-database-installer 的完整 Walking Skeleton：

**Task 1 — Cargo.toml 依赖与元数据：**
- `[package]`：name=dm-database-installer, version=0.1.0, edition=2024
- `[[bin]]`：name=dm-installer, path=src/main.rs
- 14 个依赖精确版本锁定（见 SKELETON.md）
- reqwest 使用 `rustls` feature（default-features=false），无 native-tls 依赖
- Cargo.lock 生成并提交

**Task 2 — CLI 结构体与模块占位：**
- `src/cli.rs`：Cli/Commands/InstallArgs/ValidateArgs（clap derive），5 个单元测试
- `src/main.rs`：#[tokio::main] 入口，tracing EnvFilter，子命令 dispatch
- `src/config/mod.rs`：InstallConfig 完整结构体（8 字段 + Default 实现）
- `src/config/validate.rs`：validate 子命令完整实现，3 个单元测试
- `src/download/mod.rs`：fetch_dm_installer 占位（Phase 1 返回有意义错误）
- `src/install/mod.rs`：install::run 占位（Phase 2-4 填充）
- `src/ui.rs`：print_status（完整实现）+ confirm_immutable_params（占位，--defaults 模式 stdin 保护逻辑已就位）

## Verification Evidence

| 验收标准 | 结果 |
|----------|------|
| `cargo build` exit 0 | PASS |
| `cargo test` 8 个测试通过 | PASS |
| `--help` 显示 install/validate/completions | PASS |
| `install --help` 显示 --package/--checksum/--defaults/--yes | PASS |
| `install --defaults` exit 0，不读 stdin | PASS |
| `completions bash` 输出含 `_dm-installer` | PASS (3 处匹配) |
| Cargo.toml 含 name=dm-database-installer | PASS |
| Cargo.toml 含 edition=2024 | PASS |
| Cargo.toml 含 [[bin]] name=dm-installer | PASS |
| Cargo.toml reqwest 含 rustls feature | PASS |
| Cargo.toml 无 native-tls 字符串 | PASS |
| Cargo.lock 存在 | PASS |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] reqwest 0.13.4 feature 名称错误**
- **Found during:** Task 1 `cargo build`
- **Issue:** PLAN.md 和 PATTERNS.md 中写的是 `features = ["rustls-tls", "stream"]`，但 reqwest 0.13.4 的实际 feature 名为 `rustls`（不是 `rustls-tls`）。`cargo build` 报错 `reqwest does not have that feature`。
- **Fix:** 将 `"rustls-tls"` 改为 `"rustls"`，保留 `default-features = false`，确保无 OpenSSL/native-tls 依赖。
- **Files modified:** Cargo.toml
- **Commit:** 5f48e7e

**2. [Rule 2 - Enhancement] config/validate.rs 提前实现完整逻辑**
- **Found during:** Task 2
- **Issue:** PLAN 要求 `validate.rs` 含 `pub fn run(_args: &ValidateArgs) -> anyhow::Result<()> { Ok(()) }` 占位。但 PATTERNS.md 已有完整实现模式，且 config::InstallConfig 结构体已在同 Task 中创建，实现成本为零。
- **Fix:** 直接实现 TOML 读取 + 解析逻辑，附加 3 个单元测试（valid/invalid TOML + nonexistent file）。这不改变 Plan 01 的边界——validate 是 QUAL-03 需求，已在 Plan 02 计划中。
- **Files modified:** src/config/validate.rs

### Minor Behavioral Differences

- **`cargo test --lib cli::tests`** 命令在 PLAN 验证步骤中写错了（bin 项目无 lib target）；实际使用 `cargo test` 运行所有测试，结果等价（8 个测试全通过）。

## Known Stubs

以下占位函数是有意设计的，不影响 Plan 01 目标（Walking Skeleton）：

| 文件 | 函数 | 原因 | 由哪个 Plan 完成 |
|------|------|------|-----------------|
| src/install/mod.rs | `install::run` | 编排流程 | Plan 02-04 逐步填充 |
| src/download/mod.rs | `fetch_dm_installer` | URL spike 待验证 | Phase 1/2 间 spike 后填入 |
| src/ui.rs | `confirm_immutable_params` | stdin 交互逻辑 | Plan 03 完成 |

## Threat Surface Scan

无新增威胁面（Plan 01 仅创建 CLI 解析和模块骨架，无网络端点、无文件 I/O、无认证路径）。

威胁模型中 T-01-01（stdin 保护）已在 `ui::confirm_immutable_params` 实现：`skip=true` 时在任何 stdin 读取前短路 `return Ok(())`，满足 `--defaults` 模式下 curl|sh 管道场景安全要求。

## Self-Check

见下方。
