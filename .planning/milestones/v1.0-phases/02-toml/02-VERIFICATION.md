---
phase: 02-toml
verified: 2026-06-12T10:25:00Z
status: human_needed
score: 9/10 must-haves verified
overrides_applied: 0
human_verification:
  - test: "运行 `dm-installer install --config dm.toml --package /path/to/dm.iso` 完整安装流程（真实 ISO + root 环境）"
    expected: "安装器按 TOML 指定的端口/路径/page_size/charset/extent_size 完成安装，dm.ini 和实例目录创建于配置文件指定位置"
    why_human: "需要真实达梦 ISO 包 + root 权限 + Linux 环境，测试环境为 macOS，无法自动执行（INST-02 SC1/SC2 完整流程）"
  - test: "运行 `dm-installer install --config dm.toml`（不带 --defaults/--yes）后查看参数确认 UI"
    expected: "终端输出配置文件中的 PAGE_SIZE/CHARSET/CASE_SENSITIVE/EXTENT_SIZE 实际值，等待 y/n 输入"
    why_human: "需要交互式 TTY，UI 内容需肉眼验证；自动化测试只能验证 skip=false 时 confirm_immutable_params 被调用，无法验证终端显示内容（D-08）"
---

# Phase 2: TOML 配置驱动单机 Verification Report

**Phase Goal:** DBA 可通过 TOML 配置文件自定义端口、数据路径、dminit 参数，完成单机安装
**Verified:** 2026-06-12T10:25:00Z
**Status:** human_needed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| #  | Truth | Status | Evidence |
|----|-------|--------|---------|
| 1  | `dm-installer install --config dm.toml --package /path/to/dm.iso` 读取 TOML 并按参数执行（INST-02 SC1） | ? UNCERTAIN | `resolve_config(args)?` 连接确认；但完整安装需真实 ISO + root，无法自动完成（human_needed） |
| 2  | TOML 中 port/page_size/charset/extent_size/install_path/data_path/instance_name/case_sensitive 均生效（INST-02 SC2） | ✓ VERIFIED | `test_load_config_from_args_uses_file_when_some` 覆盖全字段；`validate_install_config` 枚举值域对 24 种合法组合全部通过；`resolve_config` 将配置注入安装 7 步流程 |
| 3  | `page_size=12` 触发 `配置验证失败: page_size 无效: 12；有效值为 4/8/16/32`，不执行安装（INST-02 SC3） | ✓ VERIFIED | CLI smoke test: `./target/release/dm-installer validate --config tests/fixtures/semantic_invalid.toml` 输出 `Error: 配置验证失败: page_size 无效: 12；有效值为 4/8/16/32`，exit=1；`test_validate_install_config_rejects_invalid_page_size` PASS |
| 4  | `charset=9` 触发 `配置验证失败: charset 无效: 9` 错误 | ✓ VERIFIED | `test_validate_install_config_rejects_invalid_charset` PASS；`validate_install_config` 第 83-85 行白名单检查 `[0u8, 1, 2]` |
| 5  | `extent_size=8` 触发 `配置验证失败: extent_size 无效: 8` 错误 | ✓ VERIFIED | `test_validate_install_config_rejects_invalid_extent_size` PASS；第 86-88 行白名单 `[16u8, 32]` |
| 6  | `port=0` 触发 `配置验证失败: port 无效: 0；有效范围为 1-65535` 错误 | ✓ VERIFIED | `test_validate_install_config_rejects_port_zero` PASS；第 89-91 行 `cfg.port == 0` 检查 |
| 7  | `dm-installer validate --config config.toml` 语义合法输出 `配置文件合法: <path>`，不执行安装（QUAL-03） | ✓ VERIFIED | CLI smoke test: `./target/release/dm-installer validate --config tests/fixtures/valid.toml` 输出 `配置文件合法: tests/fixtures/valid.toml`，exit=0；`validate.rs::run()` 仅调用 `load_and_validate` 后打印，无任何安装步骤 |
| 8  | 未提供 --config 时，install 继续使用 `InstallConfig::default()`，Phase 1 流程不变（D-02 回退路径） | ✓ VERIFIED | `test_load_config_from_args_uses_default_when_none` PASS；`resolve_config` 中 `None => Ok(InstallConfig::default())` |
| 9  | 提供 --config 时，参数确认 UI 展示配置文件中的值（D-08） | ? UNCERTAIN | `step_confirm_params` 接收 `config`（已由 `resolve_config` 填充），调用 `confirm_immutable_params(config, args.defaults || args.yes)`；实际 UI 显示内容需 human 验证（TTY 交互） |
| 10 | install 和 validate 共用 `config::load_and_validate()`，无重复 TOML 解析代码（D-06, D-07） | ✓ VERIFIED | `validate.rs::run()` 仅含 `super::load_and_validate(&args.config)?`，无 `toml::from_str`；`install/mod.rs::resolve_config` 调用 `crate::config::load_and_validate(path)` |

**Score:** 8/10 truths automatically verified; 2 require human verification (truths 1 and 9)

### Required Artifacts

| Artifact | Expected | Status | Details |
|---------|---------|--------|---------|
| `src/cli.rs` | `InstallArgs.config: Option<PathBuf>` 字段 | ✓ VERIFIED | 第 51 行 `pub config: Option<PathBuf>`，含 doc comment；`test_install_args_with_config`/`test_install_args_config_default_none`/`test_install_args_config_and_defaults_combined` 全部 PASS |
| `src/config/mod.rs` | `pub fn load_and_validate` 函数 | ✓ VERIFIED | 第 69-76 行，三步链完整实现（read_to_string → from_str → validate_install_config），非 stub |
| `src/config/mod.rs` | `pub fn validate_install_config` 函数 | ✓ VERIFIED | 第 79-93 行，四个枚举/范围检查均以 `anyhow::bail!` 实现，精确中文错误消息含字段名和有效值范围 |
| `src/config/validate.rs` | 调用 `load_and_validate()` 的 thin wrapper | ✓ VERIFIED | `run()` 第 8 行 `super::load_and_validate(&args.config)?`；无内联 `toml::from_str`；`test_semantic_invalid_fixture_rejected` PASS |
| `src/install/mod.rs` | `match &args.config` 条件分支 | ✓ VERIFIED | `resolve_config()` 第 14-18 行 `match &args.config { Some(path) => ..., None => ... }`；`run()` 第 27 行 `let config = resolve_config(args)?` |
| `tests/fixtures/semantic_invalid.toml` | `page_size = 12` 语义非法 fixture | ✓ VERIFIED | 内容严格为 `page_size = 12\n`（单行加换行符） |

### Key Link Verification

| From | To | Via | Status | Details |
|------|-----|-----|--------|---------|
| `src/install/mod.rs::run()` | `src/config/mod.rs::load_and_validate()` | `resolve_config(args)?` → `match &args.config { Some(path) => crate::config::load_and_validate(path), ... }` | ✓ WIRED | `install/mod.rs` 第 15-17 行直接调用；smoke test 证明 install --config 路径打通 |
| `src/config/validate.rs::run()` | `src/config/mod.rs::load_and_validate()` | `super::load_and_validate(&args.config)?` | ✓ WIRED | `validate.rs` 第 8 行；CLI smoke test 端到端验证两条路径（合法/非法）均正常 |
| `src/config/mod.rs::load_and_validate()` | `src/config/mod.rs::validate_install_config()` | 三步链末尾 `validate_install_config(&cfg)?` | ✓ WIRED | `config/mod.rs` 第 74 行；9 个 `load_and_validate` 相关测试全部覆盖此链 |
| `src/cli.rs::InstallArgs` | `src/install/mod.rs::run()` | `args.config` 字段读取 | ✓ WIRED | `install/mod.rs` 第 15 行 `&args.config`；`make_args_no_config()` 构造器显示字段已集成 |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|---------|--------------|--------|-------------------|--------|
| `src/install/mod.rs::run()` | `config: InstallConfig` | `resolve_config(args)?` → `load_and_validate(path)` 或 `InstallConfig::default()` | 是（TOML 文件或硬编码默认值） | ✓ FLOWING |
| `src/config/validate.rs::run()` | 无渲染状态（仅验证后打印路径） | `load_and_validate(&args.config)?` | 是（读文件 + 反序列化 + 语义验证） | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|---------|--------|--------|--------|
| validate 合法 TOML 输出"配置文件合法" | `./target/release/dm-installer validate --config tests/fixtures/valid.toml` | `配置文件合法: tests/fixtures/valid.toml`，exit=0 | ✓ PASS |
| validate 语义非法 TOML 输出精确错误消息并拒绝 | `./target/release/dm-installer validate --config tests/fixtures/semantic_invalid.toml` | `Error: 配置验证失败: page_size 无效: 12；有效值为 4/8/16/32`，exit=1 | ✓ PASS |
| install --config 成功加载配置并进入安装流程 | `./target/release/dm-installer install --config tests/fixtures/valid.toml --defaults` | 日志显示"开始安装达梦数据库"→"[1/7]"→"[2/7]"，在 fetch_package 处失败（无 --package，预期正常） | ✓ PASS |
| --config flag 出现在 install --help | `./target/release/dm-installer install --help \| grep -c "\-\-config"` | 1（出现 1 次） | ✓ PASS |
| 全套测试 0 failed | `cargo test` | `test result: ok. 39 passed; 0 failed; 0 ignored` | ✓ PASS |

### Requirements Coverage

| Requirement | Description | Status | Evidence |
|-------------|-------------|--------|---------|
| INST-02 | 用户可通过 TOML 配置文件安装单机达梦，支持自定义端口、数据路径、页大小、字符集、大小写敏感等所有 dminit 参数 | ? NEEDS HUMAN (PARTIAL) | SC1（完整安装流程）和 SC2（UI 展示）需 human；SC3（错误拒绝）和 SC4（回退路径）已自动验证 |
| QUAL-03 | 用户可运行 `dm-installer validate --config config.toml` 仅验证配置文件合法性，不执行实际安装 | ✓ SATISFIED | CLI smoke test 证明 exit=0 合法 / exit=1 非法；validate.rs::run() 无任何安装调用 |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|--------|---------|--------|
| （无） | — | — | — | — |

扫描结果：`src/config/mod.rs`、`src/config/validate.rs`、`src/install/mod.rs`、`src/cli.rs` 中无 `TBD`/`FIXME`/`XXX`/`unimplemented!`/`TODO`/`HACK`/`PLACEHOLDER` 标记。无空 handler 或 `return null/[]/{}` stub。

### Human Verification Required

#### 1. INST-02 SC1/SC2 完整安装流程

**Test:** 在 Linux x86_64 机器上运行 `dm-installer install --config dm.toml --package /path/to/dm.iso`，dm.toml 指定非默认端口（如 5237）、非默认 data_path、page_size=16、charset=1
**Expected:** 安装完成后 `dm.ini` 中的 PORT=5237、DATA_PATH 等参数与 TOML 文件一致；实例目录创建于 data_path 指定位置
**Why human:** 需要真实达梦 ISO 包 + root 权限 + Linux 环境；当前测试环境为 macOS，无法执行 DMInstall.bin 和 dminit

#### 2. 参数确认 UI 展示配置文件中的值（D-08）

**Test:** 运行 `dm-installer install --config dm.toml --package /path/to/dm.iso`（不带 --defaults/--yes），查看交互式确认界面
**Expected:** 终端显示来自 TOML 文件的 PAGE_SIZE=16、CHARSET=UTF-8、CASE_SENSITIVE=Y、EXTENT_SIZE=32 实际值（而非硬编码默认值），等待用户输入 y/n
**Why human:** 需要交互式 TTY 环境；代码路径已验证（`confirm_immutable_params(config, args.defaults || args.yes)` 传入来自 resolve_config 的 config），但实际界面展示需肉眼确认

### Gaps Summary

无自动化可验证的阻断性 gap。两项 human_needed 均属"需要真实硬件/root/TTY 环境"的 manual-only 场景，已在 PLAN 的 02-VALIDATION.md 中预先标注为 Manual-Only Verifications。所有代码路径、函数连接、数据流、错误消息格式均已自动验证。

---

_Verified: 2026-06-12T10:25:00Z_
_Verifier: Claude (gsd-verifier)_
