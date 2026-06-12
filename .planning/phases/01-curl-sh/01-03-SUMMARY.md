---
phase: 01-curl-sh
plan: "03"
subsystem: dm-install-core
tags:
  - rust
  - dm-install
  - xml
  - dminit
  - interactive-ui
  - tdd
dependency_graph:
  requires:
    - cli::InstallArgs (Plan 01)
    - config::InstallConfig (Plan 01)
    - install::checksum::verify_sha256 (Plan 02)
    - install::idempotent::check_existing_instance (Plan 02)
    - download::fetch_dm_installer (Plan 02)
    - ui::print_status / StatusLevel (Plan 01)
  provides:
    - install::silent_install::run (INST-01 静默安装)
    - install::silent_install::generate_install_xml (XML 生成 + 防注入)
    - install::package::extract_dminstall_bin (ISO 提取)
    - install::init::run_dminit (dminit 初始化)
    - install::init::build_dminit_command (测试辅助)
    - ui::confirm_immutable_params (INST-03 真实实现 + 测试)
    - install::run (Step 4-7 完整编排器)
  affects:
    - Plan 04: install::run Step 8 TODO 占位接口已预留
    - Plan 04: service::register_systemd_service 调用点在 install/mod.rs 注释中
tech_stack:
  added: []
  patterns:
    - TDD RED/GREEN/REFACTOR 三阶段提交（两套循环）
    - xml_escape 函数（& 优先替换防二次转义）
    - bsdtar 优先 + mount -o loop fallback（Pitfall 3）
    - build_dminit_command 抽取为纯函数便于断言（Pitfall 2）
    - step_* 私有函数分层（run 主体 < 20 行）
    - Pitfall 4 防御：if skip { return Ok(()); } 在所有 stdin 读取前
key_files:
  created:
    - src/install/silent_install.rs
    - src/install/package.rs
    - src/install/init.rs
  modified:
    - src/install/mod.rs (Step 4-7 编排 + step_* 重构)
    - src/ui.rs (新增 test_confirm_skip_returns_ok_without_stdin)
decisions:
  - "xml_escape 中 & 必须最先替换，否则 &amp; 会被二次转义为 &amp;amp;"
  - "CREATE_DB_SERVICE / STARTUP_DB_SERVICE 固定 N，服务注册由 Plan 04 的 service.rs 精确控制"
  - "install::run 拆分为 step_* 私有函数以满足 < 40 行约束，TDD 通过后 REFACTOR 提交"
  - "bsdtar 失败后 fallback 到 mount，两者均失败则 anyhow::bail!（Pitfall 3）"
metrics:
  duration: "17 minutes"
  completed_date: "2026-06-12"
  tasks_completed: 2
  files_created: 3
  files_modified: 2
---

# Phase 1 Plan 03: DM 静默安装核心

**一句话总结：** TDD 实现 ISO 提取（bsdtar + mount fallback）、XML 响应文件生成（xml_escape 防注入，CREATE_DB_SERVICE=N）、DMInstall.bin -q 调用、dminit 参数构建（等号无空格），install::run 编排器接入 Step 4-7，24 个测试全绿。

## What Was Built

完成了 Phase 1 的"DM 静默安装核心"垂直切片，实现从 ISO 包到 dminit 完成的完整安装链路：

**Task 1 — XML 生成 + xml_escape 防注入 + confirm_immutable_params 测试 (INST-03 + XML 安全)：**

- `src/install/silent_install.rs`：
  - `xml_escape(s)` — 五字符替换（`&→&amp;`, `<→&lt;`, `>→&gt;`, `"→&quot;`, `'→&apos;`），`&` 最先处理防二次转义（T-03-01）
  - `generate_install_xml(config)` — format! 宏 + NamedTempFile，`CASE_SENSITIVE` 按 bool 映射 Y/N，`CREATE_DB_SERVICE` / `STARTUP_DB_SERVICE` 固定 N
  - `run(config, extract_dir)` — 调用 `generate_install_xml` + `run_silent_install_bin`
  - `run_silent_install_bin` — `#[cfg(unix)]` chmod 0o755 + `Command::new(DMInstall.bin).arg("-q").arg(xml_path)`
  - 4 个单元测试：all_required_tags / escapes_special_chars / case_sensitive_y_n / create_db_service_is_n
- `src/ui.rs`：新增 `test_confirm_skip_returns_ok_without_stdin`（验证 skip=true 不读 stdin，Pitfall 4）

**Task 2 — ISO 提取 + dminit 命令构建 + 编排器接入 (INST-01)：**

- `src/install/package.rs`：
  - `extract_dminstall_bin(iso_path)` — `TempDir::new()` + bsdtar 优先（`is_command_available("bsdtar")`）+ mount -o loop fallback（`extract_via_mount`），Pitfall 3 合规
  - `extract_via_mount` — mount + `fs::copy(DMInstall.bin)` + umount；失败提示 "请确认以 root 运行"
  - `is_command_available` — `Command::new("which").arg(cmd)` 检测
- `src/install/init.rs`：
  - `build_dminit_command(config)` — 返回 9 元素 Vec，`[0]` = `{install_path}/bin/dminit`，`[1..]` = `KEY=value`（无空格，Pitfall 2）
  - `run_dminit(config)` — 调用 build_dminit_command，`Command::new(binary).args(parts[1..]).status()`
  - 3 个单元测试：no_spaces_in_kv / includes_all_required_keys / first_is_binary_path
- `src/install/mod.rs`：
  - Step 4-7 接入：`step_extract` → `step_confirm_params` → `step_silent_install` → `step_dminit`
  - `run` 主体 < 20 行（拆分为 `check_idempotent_early_exit` / `fetch_package` / `verify_checksum` / `step_*`）
  - Step 8 TODO 占位 + `print_status` 提示 Plan 04 接管

## Verification Evidence

| 验收标准 | 结果 |
|----------|------|
| `cargo build` exit 0 | PASS |
| `cargo test` exit 0（24 个测试全通过） | PASS |
| `cargo test install::silent_install::tests` exit 0 (≥ 4) | PASS (4 通过) |
| `cargo test install::init::tests` exit 0 (≥ 3) | PASS (3 通过) |
| `cargo test ui::tests` exit 0 (≥ 1) | PASS (1 通过) |
| `cargo run -- validate --config tests/fixtures/valid.toml` exit 0 | PASS |
| `cargo run -- install --package /tmp/nonexistent.iso --defaults` 进入 Step 4 后 fail-fast | PASS（含 bsdtar/mount 错误） |
| silent_install.rs 不含 `<CREATE_DB_SERVICE>Y` (XML 模板) | PASS |
| init.rs 无 ` = ` 在 format! KEY=value 中（Pitfall 2） | PASS |
| xml_escape 对 install_path / data_path / instance_name 全部调用 | PASS |
| install/mod.rs Step 4-7 接入编排 | PASS |
| 所有函数 < 40 行 | PASS（Python 精确验证） |

## TDD Gate Compliance

| Gate | Commit | Status |
|------|--------|--------|
| Task 1 RED (test) | b068526 | PASS — 4 个 XML 测试 + 1 个 UI 测试预期失败（todo!()） |
| Task 1 GREEN (feat) | 536062d | PASS — 全部 21 个测试通过 |
| Task 2 RED (test) | 0abc4c2 | PASS — 3 个 dminit 测试预期失败（todo!()） |
| Task 2 GREEN (feat) | 529cc22 | PASS — 全部 24 个测试通过 |
| REFACTOR | 6d6d515 | PASS — 拆分 step_* 函数，测试仍然通过 |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] install::run 函数超过 40 行**
- **Found during:** Task 2 GREEN 后验证函数长度
- **Issue:** `run` 函数主体 50 行（awk 粗略统计），超过 CLAUDE.md 和计划的 < 40 行约束
- **Fix:** REFACTOR 阶段将 7 个步骤抽取为私有 `step_*` 辅助函数，`run` 主体缩短至 < 20 行
- **Files modified:** src/install/mod.rs
- **Commit:** 6d6d515

**2. [Plan 01 遗留] ui.rs confirm_immutable_params 已实现**
- **Found during:** Task 1 开始前检查现有代码
- **Issue:** Plan 01/02 已交付完整的 `confirm_immutable_params` 实现，Plan 03 Task 1 要求"升级为完整实现"实际上无需重写
- **Fix:** Task 1 聚焦添加 `test_confirm_skip_returns_ok_without_stdin` 测试，验证 Pitfall 4 防御已就位
- **Impact:** Task 1 UI 部分无 RED 阶段（函数已存在且正确）；XML 部分正常走 RED/GREEN/REFACTOR

## Known Stubs

| 文件 | 位置 | 原因 | 由哪个 Plan 完成 |
|------|------|------|-----------------|
| src/install/mod.rs | Step 8 TODO + print_status 占位 | systemd 服务注册由 Plan 04 完成 | Plan 04 |

## Threat Surface Scan

| Threat ID | Category | Component | 处置 |
|-----------|----------|-----------|------|
| T-03-01 | Tampering | XML 注入：install_path 含 `&` `<` 等字符 | mitigate — xml_escape() 对三个路径参数全部转义；test_xml_escapes_special_chars 验证 |
| T-03-02 | Denial of Service | curl\|sh 中 stdin 已被 curl 占用 | mitigate — confirm_immutable_params(.., skip=true) 中 `if skip { return Ok(()); }` 严格在所有 stdin 读取前；test_confirm_skip_returns_ok_without_stdin 验证 |
| T-03-03 | Tampering | TOCTOU：校验 SHA-256 后 ISO 被替换 | mitigate — install::run 顺序：checksum → 立即 extract → 立即 silent_install，无可控延迟 |
| T-03-04 | Elevation of Privilege | DMInstall.bin 来自不可信源 | mitigate — 依赖 Plan 02 的 SHA-256 校验（DOWN-02） |
| T-03-05 | Tampering | dminit 参数注入 | mitigate — Phase 1 值来自 InstallConfig::default()（硬编码可信）；Command::arg 每参数独立传递，无 shell 解析 |
| T-03-06 | Information Disclosure | XML 临时文件含路径信息 | accept — Phase 1 XML 不含 SYSDBA_PWD；NamedTempFile drop 即删 |

## Self-Check: PASSED

| 检查项 | 结果 |
|--------|------|
| src/install/silent_install.rs | FOUND |
| src/install/package.rs | FOUND |
| src/install/init.rs | FOUND |
| .planning/phases/01-curl-sh/01-03-SUMMARY.md | FOUND |
| commit b068526 (Task 1 RED) | FOUND |
| commit 536062d (Task 1 GREEN) | FOUND |
| commit 0abc4c2 (Task 2 RED) | FOUND |
| commit 529cc22 (Task 2 GREEN) | FOUND |
| commit 6d6d515 (REFACTOR) | FOUND |
