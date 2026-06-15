# Phase 1: curl|sh 单机安装 - Context

**Gathered:** 2026-06-12
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 1 实现 `curl | sh` 一行命令安装达梦单机实例的完整链路：

1. 安装包获取（本地路径 `--package` 或占位下载模块）
2. SHA-256 校验和验证（DOWN-02）
3. dminit 执行，含不可修改参数展示 + 用户确认（INST-03）
4. 达梦实例注册为 systemd 服务并开机自启（INST-04）
5. 幂等性检测：已有实例则提示退出（QUAL-02）
6. `validate` 子命令：仅验证配置合法性，不安装（QUAL-03）

**Phase 2 接管：** TOML 配置文件驱动的精细参数控制（INST-02）。
**Phase 4 接管：** 正式多平台发布流水线（真实 `curl | sh` URL 对外可用）。

</domain>

<decisions>
## Implementation Decisions

### 安装包获取策略 (DOWN-01 / DOWN-02)

- **D-01:** Phase 1 以 `--package /path/to/dm.iso` 本地路径为主交付路径。自动下载（DOWN-01）通过一个占位 `download` 模块骨架实现——能跑通流程，下载 URL 待 spike 验证可行性后填入。这样 Phase 1 的完整链路（校验 → 安装 → 注册服务）可跑通，不被下载 URL 问题阻塞。
- **D-02:** SHA-256 校验（DOWN-02）作为独立步骤，使用 `sha2` crate；本地路径 + 下载路径都经过校验，不绕过。

### CLI 入口结构

- **D-03:** 主命令 `dm-installer install [--package <path>] [--defaults]`；未传 `--package` 时尝试自动下载（占位）。`dm-installer validate --config <file>` 作为独立子命令（QUAL-03）。
- **D-04:** `--defaults` 跳过所有交互确认（供 `curl | sh` 脚本使用）；后续 Phase 2 的 `--config <toml>` 加在 `install` 子命令上。

### INST-03 不可修改参数确认流程

- **D-05:** 默认行为：安装前打印四个不可修改参数的当前值（PAGE_SIZE / CHARSET / CASE_SENSITIVE / EXTENT_SIZE）并等待 `y/n` 用户确认；输入 `n` 则 abort。
- **D-06:** `--defaults` 或 `--yes` flag 跳过确认，直接继续。`curl | sh` bootstrap 脚本自动传入 `--defaults`，保证管道场景无交互阻塞。

### curl|sh 默认安装参数

- **D-07:** 遵循 DM 官方默认值：
  - PAGE_SIZE=8, EXTENT_SIZE=16, CHARSET=GB18030, CASE_SENSITIVE=Y
  - 安装路径：`/opt/dmdbms`
  - 端口：5236
  - 实例名：DMSERVER
- 这些默认值在 `--defaults` 模式下无需用户确认即使用，在交互模式下作为展示值让用户知晓。

### 幂等性检测 (QUAL-02)

- **D-08:** 安装开始前检测 `/opt/dmdbms/dm.ini` 是否存在。存在则打印提示信息（"已检测到达梦实例，跳过安装"）并以 exit code 0 退出，不执行任何安装操作。不覆盖，不崩溃。

### Claude's Discretion

- 日志/进度展示：使用 `indicatif` 进度条（下载）+ `console` 状态消息（安装步骤）；`--verbose` 开启 tracing debug 输出。
- 错误处理：`anyhow` 用于顶层，`thiserror` 用于 download / install 模块的类型化错误。
- 服务注册（INST-04）：Linux 写 systemd unit file 到 `/etc/systemd/system/dmserver.service`，执行 `systemctl enable --now dmserver`；Windows 留占位（Phase 4 处理）。

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 需求与路线图
- `.planning/REQUIREMENTS.md` — Phase 1 需求 INST-01、INST-03、INST-04、DOWN-01、DOWN-02、QUAL-02、QUAL-03 的完整描述和验收标准
- `.planning/ROADMAP.md` §Phase 1 — 阶段目标、成功标准（5 条）、依赖关系
- `.planning/PROJECT.md` §Constraints — 技术栈约束（Rust、TOML、rustls-tls、russh、无 C FFI 依赖）

### 技术参考
- `CLAUDE.md` §Technology Stack — 推荐库版本列表（clap 4.6.1、tokio 1.52.3、reqwest 0.13.4、sha2 0.11.0、indicatif 0.18.4 等）
- `CLAUDE.md` §DM Silent Installation Integration — dminit 参数、XML response file 格式、`-q` flag 使用方式
- `CLAUDE.md` §What NOT to Use — 禁止使用 ssh2（C FFI）、native-tls、openssl、structopt

### 已知约束
- STATE.md §Blockers: DOWN-01 自动下载达梦官网安装包存在 P2 风险（无公开直链），Phase 1 主路径用 `--package` 本地包，下载功能占位

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/main.rs`: 仅有 `fn main() { println!("Hello, world!") }` 脚手架，无现有逻辑可复用

### Established Patterns
- 尚无已建立模式（首个阶段）。参照 `CLAUDE.md` §Stack Patterns by Variant 的设计模式。

### Integration Points
- Phase 1 输出的模块边界（download、install、service、config）将成为 Phase 2 TOML 配置扩展的基础；`install` 模块接口设计时需预留 `InstallConfig` 结构体让 Phase 2 填充。

</code_context>

<specifics>
## Specific Ideas

- `curl | sh` bootstrap 脚本（Phase 4 交付正式版，Phase 1 开发测试时手工调用二进制）调用约定：`dm-installer install --defaults`
- dminit 参数确认 UI 草图：
  ```
  ⚠  以下参数安装后不可修改：
     PAGE_SIZE        : 8
     CHARSET          : GB18030
     CASE_SENSITIVE   : Y
     EXTENT_SIZE      : 16
  
  确认继续安装？[y/N]
  ```
- STATE.md 明确建议：先 spike 验证达梦官网是否有机器可访问的直链，然后再做自动下载；Phase 1 不阻塞在此。

</specifics>

<deferred>
## Deferred Ideas

- **自动下载 URL** (DOWN-01 完整实现) — 需要 spike 验证达梦官网直链可行性；Phase 1 留占位，spike 完成后在 Phase 1/2 间填入
- **Windows 服务注册** (INST-04 Windows 分支) — Phase 4 处理（跨平台发布时一起实现）
- **断点续传** (DOWN-V2-01) — v2 需求，不在当前路线图范围
- **`--dry-run` 模式** (OPS-V2-02) — v2 需求

</deferred>

---

*Phase: 1-curl|sh 单机安装*
*Context gathered: 2026-06-12*
