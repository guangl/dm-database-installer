# Phase 2: TOML 配置驱动单机 - Context

**Gathered:** 2026-06-12
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 2 为 Rust 二进制 `dm-installer` 添加 TOML 配置文件驱动能力：

1. `install` 子命令新增 `--config <path>` flag，读取 TOML 文件覆盖默认参数
2. `config::load_and_validate()` 函数：TOML 反序列化 + 语义验证，`install` 和 `validate` 共用
3. 语义验证：page_size ∈ {4,8,16,32}、charset ∈ {0,1,2}、extent_size ∈ {16,32}、port ∈ [1,65535]
4. 参数确认 UI 在 `--config` 下仍然展示（用配置文件里的实际值），除非 `--yes`/`--defaults`

**Phase 1 接管的基础：** `InstallConfig` 结构体、`config/validate.rs` 解析骨架、`install/mod.rs` 流程编排。
**Phase 3 接管：** SSH 远程节点操作，主备集群配置（`[cluster]` TOML 段）。

</domain>

<decisions>
## Implementation Decisions

### install --config 整合

- **D-01:** `InstallArgs` 新增 `#[arg(long)] pub config: Option<PathBuf>`。Phase 1 D-04 已明确这是 install 子命令的 flag。
- **D-02:** `install/mod.rs::run()` 逻辑：`config` 存在时调 `config::load_and_validate(path)`，否则用 `InstallConfig::default()`；两路在同一 `run()` 函数内合并，无需拆子命令。
- **D-03:** `--config` 和 `--defaults` 可同时使用：`--config` 决定参数来源，`--defaults` 控制是否跳过确认 UI。两者正交。

### 配置字段语义验证

- **D-04:** 新增 `config::validate_install_config(cfg: &InstallConfig) -> Result<()>` 函数，在 `load_and_validate()` 内调用。验证规则：
  - `page_size` ∈ {4, 8, 16, 32}，错误信息：`"page_size 无效: {v}；有效值为 4/8/16/32"`
  - `charset` ∈ {0, 1, 2}，错误信息：`"charset 无效: {v}；有效值 0=GB18030 1=UTF-8 2=EUC-KR"`
  - `extent_size` ∈ {16, 32}，错误信息：`"extent_size 无效: {v}；有效值为 16/32"`
  - `port` ∈ [1, 65535]（u16 类型本身已约束，无需额外检查）
- **D-05:** 路径字段（`install_path`、`data_path`）不做存在性校验（安装器负责创建）；但需非空字符串——serde 反序列化已隐式保证非空。

### validate 与 install 验证代码复用

- **D-06:** `config::load_and_validate(path: &Path) -> Result<InstallConfig>`：读取文件 → `toml::from_str` → `validate_install_config()`，三步合一。两个调用点：
  - `install --config <path>`：调 `load_and_validate()` 取 `InstallConfig`，然后继续安装流程
  - `validate --config <path>`：调 `load_and_validate()` 后打印 "配置文件合法: {path}"
- **D-07:** Phase 1 `config/validate.rs::run()` 重构为调用 `load_and_validate()`，去除重复解析代码。

### 参数确认 UI 与 --config 联动

- **D-08:** 提供 `--config` 时，`ui::confirm_immutable_params()` 仍然展示四个不可修改参数的值（来自 config 文件，非硬编码默认值），并等待 `y/n` 确认。
- **D-09:** 仅当用户显式传 `--yes` 或 `--defaults` 时跳过确认。`--config` 本身不自动跳过——用户写完配置文件后仍需"最后确认"不可修改参数。

### Claude's Discretion

- TOML 字段名沿用 `InstallConfig` 的字段名（snake_case），与 serde 默认行为一致，无需 `#[serde(rename)]`。
- 语义验证错误用 `anyhow::bail!()` 格式化，不新增 `thiserror` 错误类型（验证是 application-level 逻辑）。
- `validate_install_config()` 是纯函数，不做 I/O，便于单元测试覆盖所有边界值。

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 需求与路线图

- `.planning/REQUIREMENTS.md` — INST-02（TOML 配置驱动单机，含 SC1-SC4）、QUAL-03（validate 子命令）完整描述和验收标准
- `.planning/ROADMAP.md` §Phase 2 — 阶段目标、成功标准（4 条）、依赖关系
- `.planning/PROJECT.md` §Constraints — 技术栈约束（Rust、TOML、rustls-tls）

### 先前阶段决策

- `.planning/phases/01-curl-sh/01-CONTEXT.md` — Phase 1 的 CLI 结构（D-03 D-04）、默认参数（D-07）、幂等性检测（D-08）；Phase 2 在这些决策之上扩展，不推翻。

### 技术参考

- `CLAUDE.md` §Technology Stack — 推荐库版本（clap 4.6.1、serde 1.0.228、toml 1.1.2、anyhow 1.0.102）
- `CLAUDE.md` §DM Silent Installation Integration — dminit 参数名和有效值范围（PAGE_SIZE/CHARSET/CASE_SENSITIVE/EXTENT_SIZE）
- `CLAUDE.md` §What NOT to Use — 禁止 openssl / native-tls / ssh2

### 已有代码

- `.claude/worktrees/agent-a693079c0c4cadfbf/src/config/mod.rs` — `InstallConfig` 结构体（所有字段、serde default 函数）；Phase 2 在此基础上添加 `load_and_validate()`
- `.claude/worktrees/agent-a693079c0c4cadfbf/src/config/validate.rs` — Phase 1 `validate` 子命令实现；Phase 2 重构为调用共用函数
- `.claude/worktrees/agent-a693079c0c4cadfbf/src/cli.rs` — `InstallArgs` 结构体；Phase 2 新增 `--config` field
- `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/mod.rs` — `run()` 编排器；Phase 2 改 `InstallConfig::default()` 为条件分支

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets

- `InstallConfig`（`config/mod.rs`）：所有字段已有 serde `default` 函数和 `Default` impl；Phase 2 直接 `toml::from_str::<InstallConfig>()` 即可，serde 自动回退默认值
- `config/validate.rs::run()`：已有 TOML 解析 + `InstallConfig` 反序列化逻辑；Phase 2 提取为共用函数并加语义验证
- `install/mod.rs::run()`：已有完整安装编排（7步），Phase 2 只需修改第一步的配置获取逻辑
- `ui::confirm_immutable_params(config, skip)`：接受 `&InstallConfig` 和 `bool`，Phase 2 无需改动 UI 函数签名

### Established Patterns

- 错误处理：顶层 `anyhow::Result<()>`，模块内错误用 `anyhow::bail!()` + `.with_context(|| ...)`
- CLI 参数：clap derive macro，`#[arg(long)]` 模式；global flags 用 `#[arg(global = true)]`
- 日志：`tracing::info!` 在每个安装步骤开始处记录进度（格式：`"[N/7] 步骤名"`）

### Integration Points

- `cli::InstallArgs` → 新增 `config: Option<PathBuf>` 字段
- `install::run(args: &InstallArgs)` → 入口处根据 `args.config` 决定配置来源
- `config::validate::run()` → 重构为调用 `config::load_and_validate()`

</code_context>

<specifics>
## Specific Ideas

- TOML 配置文件示例（下游 agent 可直接用于测试 fixture）：
  ```toml
  install_path = "/opt/dmdbms"
  data_path = "/opt/dmdbms/data"
  instance_name = "DMSERVER"
  port = 5237
  page_size = 16
  charset = 1
  case_sensitive = true
  extent_size = 32
  ```
- 语义验证错误消息示例（INST-02 SC3 的具体实现）：
  ```
  配置验证失败: page_size 无效: 12；有效值为 4/8/16/32
  ```
- `validate` 成功输出格式（与 Phase 1 保持一致）：
  ```
  配置文件合法: /path/to/config.toml
  ```

</specifics>

<deferred>
## Deferred Ideas

- **[cluster] TOML 段落** — 主备集群节点列表、SSH 凭据、dm.ini 分发逻辑；Phase 3 范围
- **`--dry-run` 模式** (OPS-V2-02) — v2 需求，不在当前路线图
- **断点续传** (DOWN-V2-01) — v2 需求
- **自动下载 URL** (DOWN-01) — 待 spike 验证达梦官网直链可行性（STATE.md P2 风险）

</deferred>

---

*Phase: 2-TOML 配置驱动单机*
*Context gathered: 2026-06-12*
