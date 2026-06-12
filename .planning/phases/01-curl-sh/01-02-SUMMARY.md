---
phase: 01-curl-sh
plan: "02"
subsystem: validation-idempotency
tags:
  - rust
  - sha256
  - validation
  - idempotency
  - toml
  - tdd
dependency_graph:
  requires:
    - cli::InstallArgs
    - cli::ValidateArgs
    - config::InstallConfig
  provides:
    - config::validate::run (QUAL-03)
    - install::checksum::verify_sha256 (DOWN-02)
    - install::idempotent::check_existing_instance (QUAL-02)
    - download::fetch_dm_installer (DOWN-01 骨架)
    - install::run (Plan 02 编排器)
  affects:
    - Plan 03: 通过 InstallConfig::default() 获取 D-07 默认值
    - Plan 03: install::run 编排器 TODO 接口预留
tech_stack:
  added:
    - sha2 0.11.0 (SHA-256 文件校验)
  patterns:
    - TDD RED/GREEN/REFACTOR 三阶段提交
    - anyhow::Context 错误链（中文上下文消息）
    - Path::new(config.install_path).join("dm.ini") 参数化路径（Pitfall 6 合规）
    - serde(default = "fn") 函数级默认值
    - tokio::test 异步单元测试
key_files:
  created:
    - src/install/checksum.rs
    - src/install/idempotent.rs
    - tests/fixtures/valid.toml
    - tests/fixtures/invalid.toml
  modified:
    - src/config/validate.rs (增强测试：错误消息断言 + 2 个新测试)
    - src/download/mod.rs (添加 tokio::test 测试)
    - src/install/mod.rs (子模块声明 + 完整编排逻辑)
decisions:
  - "sha2 0.11.0 的 GenericArray 不支持 {:x} 格式化，改用 .iter().map(|b| format!(\"{:02x}\", b)).collect() 生成 hex 字符串"
  - "TDD RED 阶段使用 todo!() 占位确保测试真正失败，满足 TDD 协议要求"
  - "Plan 01 已超额交付 validate 核心逻辑，Task 1 主要是增强测试断言 + 创建 fixtures"
metrics:
  duration: "8 minutes"
  completed_date: "2026-06-12"
  tasks_completed: 2
  files_created: 4
  files_modified: 3
---

# Phase 1 Plan 02: 验证 + 幂等垂直切片

**一句话总结：** TDD 流程实现 SHA-256 文件校验（大小写不敏感）、dm.ini 幂等检测（参数化路径）、validate 子命令 5 测试增强、下载占位 tokio::test，install::run 编排器串联 3 步，16 个测试全绿。

## What Was Built

完成了 Phase 1 的"验证 + 幂等"垂直切片，为用户提供两条无副作用的可运行路径：

**Task 1 — InstallConfig 验证 + validate 子命令 (QUAL-03)：**
- 增强 `config/validate.rs` 测试：将 `is_err()` 断言升级为消息内容断言（含"配置文件解析失败"/"无法读取配置文件"）
- 新增 `test_install_config_defaults`：验证 D-07 规定的全部 8 个默认值
- 新增 `test_install_config_partial_toml`：验证 serde `default = "fn"` 属性生效（仅覆盖 port，其余字段保持默认）
- 创建 `tests/fixtures/valid.toml`（port=5237, page_size=16）和 `tests/fixtures/invalid.toml`（port="not_a_number"）

**Task 2 — SHA-256 + 幂等检测 + 下载占位 + install 编排 (DOWN-01/02, QUAL-02)：**
- `src/install/checksum.rs`：`verify_sha256(path, expected_hex)` — 65536 字节分块读取，`to_lowercase()` 大小写不敏感比较，`iter().map(|b| format!("{:02x}", b))` hex 转换（sha2 0.11.0 GenericArray 兼容方式）
- `src/install/idempotent.rs`：`check_existing_instance(config)` — `Path::new(&config.install_path).join("dm.ini").exists()`（Pitfall 6 合规：无硬编码路径）
- `src/download/mod.rs`：添加 `#[tokio::test] async fn test_fetch_stub_returns_error`，验证占位消息包含"Phase 1 占位"和"--package"
- `src/install/mod.rs`：完整编排器——Step 1 幂等检测、Step 2 包路径获取（None 调用 download 占位失败）、Step 3 checksum 校验（None 时 `tracing::warn!` 跳过）、TODO Plan 03/04 注释

## Verification Evidence

| 验收标准 | 结果 |
|----------|------|
| `cargo test --lib config::validate::tests` exit 0 (≥5 tests) | PASS (5 通过) |
| `cargo run -- validate --config tests/fixtures/valid.toml` exit 0，含"配置文件合法" | PASS |
| `cargo run -- validate --config tests/fixtures/invalid.toml` exit ≠ 0 | PASS |
| `cargo run -- validate --config /nonexistent/x.toml` stderr 含"无法读取配置文件" | PASS |
| `cargo test --lib install::checksum::tests` exit 0 (≥3 tests) | PASS (3 通过) |
| `cargo test --lib install::idempotent::tests` exit 0 (≥2 tests) | PASS (2 通过) |
| `cargo test --lib download::tests` exit 0 (≥1 test) | PASS (1 通过) |
| `cargo run -- install`（无 --package）exit ≠ 0，含"--package" | PASS |
| `cargo run -- install --package /tmp/nonexistent.iso --defaults` exit 0 | PASS |
| idempotent.rs 不含硬编码 "/opt/dmdbms" | PASS |
| 全部 16 个测试通过 | PASS |

## TDD Gate Compliance

| Gate | Commit | Status |
|------|--------|--------|
| RED (test) | bd7dedb | PASS — 5 个新测试全部失败（todo!() 占位） |
| GREEN (feat) | 318136d | PASS — 16 个测试全部通过 |
| REFACTOR | 无需单独提交 | PASS — 实现清晰，函数均 < 40 行 |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] sha2 0.11.0 GenericArray 不支持 {:x} 格式化**
- **Found during:** Task 2 GREEN — 编写 `compute_sha256` 实现后 `cargo build` 报 `E0277: the trait bound Array<u8, ...>: LowerHex is not satisfied`
- **Issue:** sha2 0.11.0 使用 digest 0.11.x，`hasher.finalize()` 返回 `GenericArray<u8, N>`，该类型未实现 `LowerHex` trait，无法直接 `format!("{:x}", ...)`
- **Fix:** 改用 `hash_bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>()` 生成 hex 字符串
- **Files modified:** src/install/checksum.rs
- **Commit:** 318136d

**2. [Rule 2 - Enhancement] Plan 01 超额交付，Task 1 测试直接进入 GREEN**
- **Found during:** Task 1 开始前检查现有代码
- **Issue:** Plan 01 已实现完整 validate 逻辑（3 个测试），Plan 02 Task 1 要求实现的核心功能已存在
- **Fix:** Task 1 聚焦测试增强（错误消息断言 + 2 个新测试）+ fixtures 创建；validate 核心逻辑无需重写
- **Impact:** Task 1 无 RED 阶段（测试新增后直接通过），符合 TDD 精神（已绿代码不需重写）

## Known Stubs

| 文件 | 函数 | 原因 | 由哪个 Plan 完成 |
|------|------|------|-----------------|
| src/install/mod.rs | Step 4-7 TODO 注释 | ISO 提取/参数确认/DMInstall.bin/systemd | Plan 03-04 |
| src/ui.rs | `confirm_immutable_params` | stdin 交互逻辑 | Plan 03 |

## Threat Surface Scan

T-02-01（路径遍历）：`checksum::verify_sha256` 仅做文件二进制 hash，不解析路径，不引入新风险。
T-02-02（校验绕过）：`install::run` 中 `--checksum` 为 None 时 `tracing::warn!` 跳过，符合 CONTEXT.md Open Question 1 产品决策。
T-02-03/T-02-04：`validate` 和 `read_to_string` 行为无变化，威胁处置不变。

无新增威胁面超出计划的 threat_model。

## Self-Check: PASSED

| 检查项 | 结果 |
|--------|------|
| src/install/checksum.rs | FOUND |
| src/install/idempotent.rs | FOUND |
| tests/fixtures/valid.toml | FOUND |
| tests/fixtures/invalid.toml | FOUND |
| .planning/phases/01-curl-sh/01-02-SUMMARY.md | FOUND |
| commit 458faab (Task 1) | FOUND |
| commit bd7dedb (Task 2 RED) | FOUND |
| commit 318136d (Task 2 GREEN) | FOUND |
