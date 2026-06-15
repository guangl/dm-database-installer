---
phase: 07-dsc
verified: 2026-06-15T05:59:59Z
status: human_needed
score: 13/14 must-haves verified
overrides_applied: 0
human_verification:
  - test: "运行 `cargo run -p dm-database-installer -- install dsc --help` 并确认帮助文本不报错（实际应通过 config.toml 的 install_type = \"dsc\" 触发 DSC 模式）"
    expected: "命令不报错，或输出表明 dsc 通过 config.toml 配置；缺 dsc 配置时错误消息应含 dsc.toml 或 dsc_storage 而非 panic"
    why_human: "CLI 实际入口通过 config.toml 的 install_type 字段触发 DSC，不是 install dsc 子命令；需要人工构建 dsc.toml 验证错误提示"
  - test: "准备含有效 dsc.toml（oguid + [dsc_storage] + [[nodes]] primary/standby）后运行 install，确认 SSH 连接失败时错误消息含主机连接失败，而非 'DSC 集群部署尚未实现'"
    expected: "错误消息含 '连接节点' 或 SSH 相关内容，不含 '尚未实现'"
    why_human: "需要真实 dsc.toml 文件和网络环境触发 SSH 连接阶段"
  - test: "若有真实 2 节点 + 4 块共享块设备环境，执行完整部署并验证：a) 10 个阶段日志输出；b) 中断后重跑产生 '[续] 跳过 xxx' 日志；c) 完成后所有节点 V$INSTANCE STATUS$=OPEN"
    expected: "部署成功，日志显示阶段标志，断点续传跳过已完成步骤，所有节点 OPEN 状态"
    why_human: "需要真实 DSC 硬件环境（共享块设备 + 两台节点），无法在开发机上模拟"
---

# Phase 7: DSC 共享存储集群 验证报告

**Phase Goal:** 实现 DSC（数据共享集群）部署支持，dm-installer 能读取 DSC 集群配置并编排完整的多节点部署流程（安装→配置分发→DMCSS/DMASM启动→ASM初始化→dminit→dmserver启动→验证），支持断点续传。
**Verified:** 2026-06-15T05:59:59Z
**Status:** human_needed
**Re-verification:** No — 初次验证

## Goal Achievement

### Observable Truths

| #  | Truth | Status | Evidence |
|----|-------|--------|---------|
| 1  | 用户在 dsc.toml 中通过 [dsc_storage] 提供四个块设备路径，配置加载即可成功 | VERIFIED | `DscStorageConfig` struct 存在，`ClusterSpecificConfig.dsc_storage: Option<DscStorageConfig>`，test_dsc_accepts_dsc_storage 通过 |
| 2  | 若缺少 [dsc_storage] 段，load_cluster_specific 返回含 "dsc_storage" 的错误 | VERIFIED | `validate_dsc` 在 `cfg.dsc_storage.is_none()` 时 bail!("...必须配置 [dsc_storage]...")，test_dsc_requires_dsc_storage 通过 |
| 3  | ClusterCheckpoint 反序列化旧版 JSON 时 DSC 字段默认 false，向前兼容 | VERIFIED | 所有 6 个新字段均有 `#[serde(default)]`，test_old_checkpoint_file_still_loads 通过 |
| 4  | ClusterCheckpoint 6 个 DSC 字段（dsc_config_distributed 等）正确 roundtrip | VERIFIED | test_dsc_checkpoint_roundtrip 通过，save_to + load_from 行为确认 |
| 5  | dmdcr_cfg.ini 含 CSS/ASM/DB 三个 [GRP] 段，N 节点对应正确端口 | VERIFIED | `generate_dmdcr_cfg_ini` 实现，9 个相关单元测试通过（含 CSS 端口 9341/9343、ASM 端口 9349/9351） |
| 6  | dmdcr.ini 按节点 index 生成不同 DMDCR_SEQNO（Pitfall 3 防御） | VERIFIED | test_dmdcr_ini_seqno_differs_per_node 通过，SEQNO=0 vs SEQNO=1 |
| 7  | dminit.ini 的 SYSTEM_PATH/LOG_PATH 以 + 开头（Pitfall 4 防御） | VERIFIED | `generate_dminit_ini` 硬编码 "+DMDATA/data" 和 "+DMLOG/log/..."，test_dminit_ini_asm_path_prefix 通过 |
| 8  | DSC 安装阶段调用 upload_installer_and_install，不调用 run_dminit_remote（Pitfall 1 防御） | VERIFIED | `run_dsc_install_only` 仅委托 upload_installer_and_install，grep 确认 run_dminit_remote 调用次数 = 0，test_run_dsc_install_only_calls_upload_but_not_dminit 通过 |
| 9  | 配置分发函数推送 3 个文件（dmdcr_cfg/dmasvrmal/dmdcr）到各节点，dmdcr SEQNO 按节点 index | VERIFIED | `distribute_dsc_configs` 实现，test_distribute_dsc_configs_uploads_three_files 和 test_distribute_dsc_configs_dmdcr_seqno_matches_index 通过 |
| 10 | DMCSS → DMASM 顺序启动（Pitfall 2 防御），等待 DMASM 端口就绪后才执行 dmasmtool | VERIFIED | `run_start_css_asm_all_nodes` 先并行 DMCSS 后并行 DMASM，再 health_check_fn(:9349)；test_register_and_start_dmcss_service/dmasm_service 通过 |
| 11 | dminit 通过 control file 在 first_node 执行（Pitfall 4），config 目录通过 tar+SFTP 分发到 other_nodes | VERIFIED | `run_dminit_shared` 生成 dminit.ini 上传后执行 `dminit control={path}`，`distribute_config_dir` 实现 tar+SFTP 流程，相关 MockRunner 测试通过 |
| 12 | V$INSTANCE 验证检查 STATUS$=OPEN（Pitfall 5，不强制 PRIMARY 字样） | VERIFIED | `verify_dsc_node` 断言 `output.to_uppercase().contains("OPEN")`，test_verify_dsc_node_accepts_open_normal / rejects_other_status 通过 |
| 13 | dm-installer 入口不再 bail!("DSC 集群部署尚未实现")，按 8 个 checkpoint gate 顺序编排全流程 | VERIFIED | grep 确认该字符串出现次数 = 0；run_with_runners 含 8 个 Gate；test_run_with_runners_calls_steps_in_order_no_checkpoint 验证调用顺序 |
| 14 | 断点续传：8 个 DSC gate 字段接入 checkpoint，重跑时产生 "[续] 跳过" 日志 | VERIFIED (partial) | 代码中 6 个 DSC gate 字段接入（加 preflight_done/install_done 共 8 个），test_run_with_runners_skips_completed_steps_from_checkpoint 通过；"[续] 跳过" 字样在 tracing::info! 中存在；真实中断重跑日志需人工验证 |

**Score:** 13/14 truths verified（Truth 14 技术实现已验证，真实中断场景需人工确认）

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/config/cluster.rs` | DscStorageConfig 结构、ClusterSpecificConfig.dsc_storage 可选字段、validate_dsc 校验扩展 | VERIFIED | `pub struct DscStorageConfig` 存在，4 个磁盘字段，Default 实现 /dev/raw/raw1~raw4，validate_dsc 检查 dsc_storage 存在性+互异+非空 |
| `src/cluster/checkpoint.rs` | ClusterCheckpoint 扩展 6 个 DSC 字段 | VERIFIED | dsc_config_distributed/css_asm_started/asm_diskgroup_created/dminit_shared_done/config_dir_distributed/dmserver_started 全部存在，均有 `#[serde(default)]` |
| `src/cluster/dsc/templates.rs` | 4 个 INI 生成函数（>= 150 行） | VERIFIED | generate_dmdcr_cfg_ini/dmasvrmal_ini/dmdcr_ini/dminit_ini 全部实现，文件约 400 行，16 个单元测试覆盖 |
| `src/cluster/dsc/mod.rs` | 声明 templates/deploy 子模块，完整 run()/run_with_runners() | VERIFIED | `pub mod templates; pub mod deploy;` 存在，run() 建立 SSH 后委托 run_with_runners()，run_with_runners() 含 8 个 gate |
| `src/cluster/dsc/deploy.rs` | 10 个 pub async 部署函数（>= 300 行） | VERIFIED | run_dsc_install_only/distribute_dsc_configs/register_and_start_dmcss_service/register_and_start_dmasm_service/register_and_start_dmserver_service/run_dmasmcmd_init/run_dmasmtool_create_diskgroups/run_dminit_shared/distribute_config_dir/verify_dsc_node 全部实现，文件约 600 行，14 个 MockRunner 测试 |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| src/config/cluster.rs | validate_dsc | dsc_storage.is_none() 检查 | VERIFIED | 第 334 行 `if cfg.dsc_storage.is_none()` |
| src/cluster/checkpoint.rs | serde::Deserialize | #[serde(default)] 全部新字段 | VERIFIED | 6 个新字段均有 `#[serde(default)]` 属性 |
| src/cluster/dsc/templates.rs | DscStorageConfig | use crate::config::cluster 导入 | VERIFIED | 第 4 行导入 DscStorageConfig，4 个函数使用 storage 参数 |
| src/cluster/dsc/mod.rs | deploy.rs | `deploy::` 前缀调用全部 10 个函数 | VERIFIED | grep 确认 deploy::run_dsc_install_only/distribute_dsc_configs/register_and_start_dmcss_service 等全部引用 |
| src/cluster/dsc/mod.rs | ClusterCheckpoint | 8 个 gate 字段（dsc_config_distributed 等）read+save | VERIFIED | 每个 gate 含 `!cp.xxx { ... cp.xxx = true; cp.save()?; }` 结构，共 15 次引用 |
| src/cluster/dsc/mod.rs | phases::run_preflight | Gate 1 调用 | VERIFIED | `phases::run_preflight(&runners, &dminit).await?` 存在 |
| src/cluster/dsc/mod.rs | phases::run_install_phase | 不调用（Pitfall 1）| VERIFIED | grep 确认出现次数 = 0 |
| src/cluster/dsc/deploy.rs | shell_quote() | 所有路径参数包裹 | VERIFIED | shell_quote 调用次数 = 25，超过计划要求的 8 次 |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| src/cluster/dsc/mod.rs | storage (DscStorageConfig) | specific.dsc_storage.as_ref().ok_or_else() | 来自 TOML 配置解析 | FLOWING |
| src/cluster/dsc/mod.rs | cp (ClusterCheckpoint) | ClusterCheckpoint::load()?.unwrap_or_default() | 来自 JSON 文件或默认值 | FLOWING |
| src/cluster/dsc/templates.rs | generate_dmdcr_cfg_ini 输出 | nodes/oguid/storage/dminit 参数 | 纯函数，动态生成 INI 内容 | FLOWING |
| src/cluster/dsc/deploy.rs | dmdcr_content (dmdcr.ini) | generate_dmdcr_ini(node_index, ...) | SEQNO 按 node_index 动态生成 | FLOWING |
| src/cluster/dsc/deploy.rs | verify_dsc_node stdout | disql V$INSTANCE 查询结果 | 来自 SSH exec 远程命令输出 | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| 全套 DSC 单元测试 | cargo test --bin dm-installer cluster::dsc | 35 passed; 0 failed | PASS |
| DSC 配置校验测试 | cargo test --bin dm-installer config::cluster::tests::test_dsc | 5 passed; 0 failed | PASS |
| ClusterCheckpoint DSC 字段测试 | cargo test --bin dm-installer cluster::checkpoint | 6 passed; 0 failed | PASS |
| Release 构建 | cargo build --release -p dm-database-installer | Finished without warnings | PASS |
| 全套测试 | cargo test --bin dm-installer | 246 passed; 0 failed | PASS |
| `bail!("DSC 集群部署尚未实现")` 是否已移除 | grep 代码库 | 出现次数 = 0 | PASS |
| phases::run_install_phase 是否未被调用 | grep dsc/mod.rs | 出现次数 = 0（Pitfall 1 防御） | PASS |
| shell_quote 防注入使用次数 | grep deploy.rs | 25 次（>= 要求的 8 次） | PASS |

### Probe Execution

Step 7c: SKIPPED — 项目无 scripts/*/tests/probe-*.sh 探针文件，PLAN 中未声明探针。

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|---------|
| DSC-01 | 07-01/02/03/04 | 用户可执行 dm-installer install dsc 完成 DSC 共享存储集群完整部署 | VERIFIED (with human caveat) | run_with_runners() 完整实现 10 阶段流程，5 个集成单元测试验证；真实端到端需人工验证 |
| DSC-02 | 07-03/04 | 安装流程在所有节点自动调用 dmasmtool 初始化 ASM 磁盘组 | VERIFIED | run_dmasmtool_create_diskgroups 实现，run_asm_init_first_node 在 Gate 5 调用 dmasmcmd_init 后调用 dmasmtool；MockRunner 测试验证命令格式 |
| DSC-03 | 07-03/04 | 第一节点执行 dminit（路径指向共享存储），其他节点直接挂载启动 | VERIFIED | run_dminit_shared 仅在 first_node 执行（Gate 6），其余节点通过 distribute_config_dir 接收 config 目录，test 验证 dminit 不重复 |

所有 3 个 DSC 需求均已实现，无孤立需求。

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/cluster/dsc/templates.rs | 2 | `#![allow(dead_code)]` | Info | 计划中明确说明 Plan 03 引用后移除；Plan 03 已引用，但该属性未被移除（warnings 被抑制） |
| src/cluster/dsc/deploy.rs | 2 | `#![allow(dead_code)]` | Info | 同上，Plan 04 引用后应移除；Plan 04 已引用但属性仍存在 |

**说明：** 上述两处 `#![allow(dead_code)]` 属于遗留的 lint 抑制，不影响实际功能——对应函数已被 dsc/mod.rs 引用。SUMMARY.md 承认这两处是已知 debt（"Plan 03/04 引用后移除"）但实际未清理。这不是 BLOCKER（release 构建通过，功能正常），属于轻微技术债。

未发现 `TBD`、`FIXME`、`XXX` 等未引用的 debt 标记。未发现 placeholder 实现或空返回。

### Human Verification Required

#### 1. CLI 入口验证 —— DSC 模式触发方式确认

**Test:** 构建 dsc.toml（含 install_type = "dsc"、oguid、[dsc_storage]、两个 [[nodes]]），运行 `dm-installer install` 验证：(a) 配置加载成功；(b) SSH 连接失败时报 "连接节点 xxx 失败"，不含 "DSC 集群部署尚未实现"
**Expected:** 错误发生在 SSH 连接阶段，消息明确指出连接失败原因
**Why human:** 需要构建真实配置文件并在无 SSH 可达环境运行，自动化测试无法验证 CLI 路径

#### 2. 真实 DSC 环境端到端部署验证

**Test:** 在 2 节点 + 4 块共享块设备环境执行完整部署，验证：(a) 10 个阶段日志；(b) 中断后重跑输出 "[续] 跳过" 日志；(c) 所有节点 V$INSTANCE STATUS$=OPEN
**Expected:** 部署成功，日志包含阶段标志，断点续传跳过已完成步骤，所有节点 OPEN 状态
**Why human:** 需要真实 DSC 硬件（共享块设备 + 两台 Linux 节点），开发机无法模拟

### Gaps Summary

无 BLOCKER 级别的差距。所有自动化可验证的 must-have 均已通过代码审查和测试运行确认：

- 35 个 DSC 单元测试全部通过（templates + deploy + orchestration）
- 246 个全套测试全部通过
- release 构建无 warning
- `bail!("DSC 集群部署尚未实现")` 已不存在
- `phases::run_install_phase` 未在 DSC 入口调用（Pitfall 1 防御已落实）
- 8 个 checkpoint gate 字段全部接入

剩余 2 个人工验证项：(1) CLI 入口路径验证（dsc.toml 触发方式），(2) 真实 DSC 硬件环境端到端验证。这两项均为 ? UNCERTAIN（无法通过代码静态分析验证），不属于已知失败。

遗留技术债：templates.rs 和 deploy.rs 中的 `#![allow(dead_code)]` 在 Plan 03/04 引用后应已移除但未清理，建议在后续 cleanup PR 中处理。

---

_Verified: 2026-06-15T05:59:59Z_
_Verifier: Claude (gsd-verifier)_
