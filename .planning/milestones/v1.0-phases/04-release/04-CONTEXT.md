# Phase 4: 发布流水线 - Context

**Gathered:** 2026-06-13
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 4 在 Phase 1-3 的实现基础上，通过 `cargo-dist` 建立多平台发布流水线：

1. 配置 `cargo-dist` 生成 GitHub Actions release CI，在 `v*` tag 触发时自动跨平台构建
2. 构建 Linux x86_64、Linux aarch64、Windows x86_64 三个平台的 `dm-installer` 预编译二进制
3. 将 `cargo-dist` 生成的 `install.sh` bootstrap 脚本替换现有手写脚本（或作为独立的 installer 入口）
4. GitHub Releases 成为二进制的公开分发渠道（PLAT-01/02/03）
5. PLAT-04（Windows 目标机）作为 placeholder：CLI 结构就位，`setup.exe /q /XML` 实际集成留 spike

**Phase 3 接管的基础：** 完整的 `dm-installer` Rust CLI（install + cluster deploy），所有平台公共代码已实现。
**Phase 4 不做：** 达梦官网 DM8 安装包下载逻辑（已在 install.sh + versions.txt 管理），也不改变 Phase 1-3 的核心功能。

</domain>

<decisions>
## Implementation Decisions

### 发布工具

- **D-01:** 使用 `cargo-dist` 管理发布流水线（CLAUDE.md 明确推荐，与 uv/Rye 同类项目一致）。配置写入 `Cargo.toml` 的 `[workspace.metadata.dist]`（或 `dist-workspace.toml`，以 cargo-dist init 默认输出为准）。
- **D-02:** 触发条件为 `v*` tag push（如 `v0.1.0`），与 Cargo.toml `[package].version` 字段对齐。不做手动 workflow_dispatch 发布（避免二义性）。
- **D-03:** 不使用 `cargo-dist` 前，先运行 `cargo dist init` 交互配置目标平台，生成 `.github/workflows/release.yml`。现有 `update-versions.yml` 保留，只新增 release 工作流。

### 目标平台矩阵

- **D-04:** 三个构建目标（**已更新 2026-06-13**：Windows 目标从 `x86_64-pc-windows-gnu` 改为 `x86_64-pc-windows-msvc`，原因：`ring` crate issue #1363 + `mio` issue #1632 导致 windows-gnu 无法从 Linux 交叉编译，用户已授权此变更）：
  - `x86_64-unknown-linux-gnu` — PLAT-01 (Linux x86_64 控制机 + 目标机)
  - `aarch64-unknown-linux-gnu` — PLAT-02 (Linux ARM64 控制机 + 目标机)
  - `x86_64-pc-windows-msvc` — PLAT-03 (Windows 控制机，SSH 到 Linux 节点，在 windows-2022 runner 原生构建)
- **D-05:** aarch64 使用 apt 安装 `gcc-aarch64-linux-gnu` + `.cargo/config.toml` linker 配置进行交叉编译（替代原 `cross` 方案：cargo-dist 不调用 cross，使用 apt 工具链方案更直接；Windows 目标在 windows-2022 runner 原生构建，无需交叉编译）。效果等价于原 D-05 方案。
- **D-06:** `rustls-tls` feature 已在 Cargo.toml 中配置，无 OpenSSL 依赖，三个平台均可静态链接。

### PLAT-04 Windows 目标机 DM 安装

- **D-07:** PLAT-04 纳入 Phase 4 作为 placeholder：
  - CLI 层面：新增 `install windows` 子命令（或 `--target-os windows` flag），可解析 TOML 配置
  - 构建层面：`x86_64-pc-windows-msvc` target 也构建，打包进 Release
  - 实际调用 `setup.exe /q /XML <path>` 的集成逻辑标记为 `todo!()` + 文档注释，留作 spike
  - 理由：DM Windows 安装包 URL 需从 eco.dameng.com 单独验证，不阻塞其他 PLAT 需求

### install.sh bootstrap 集成

- **D-08:** `cargo-dist` 会生成一个通用 `install.sh` bootstrap 脚本，负责根据平台下载正确的 binary tarball 并安装到 `$PATH`（通常 `~/.cargo/bin` 或 `/usr/local/bin`）。
  - 这与现有 Phase 1 `install.sh`（下载 DM8 安装包并安装达梦数据库）功能不同——两个文件目的完全不同。
  - 保留现有 `install.sh` 作为 DM8 安装器脚本（Phase 1 产出）；`cargo-dist` 生成的脚本独立命名，如 `dm-installer-installer.sh` 或通过 Release 页面分发。
  - CLAUDE.md 中 `curl | sh` 单行命令指的是 Phase 1 的 DM 安装脚本，Phase 4 重点是让 `dm-installer` 二进制本身可公开下载。

### 版本号管理

- **D-09:** Cargo.toml `[package].version` 是单一事实来源；打 tag 前手动更新版本号，commit 后再 push tag。不引入 cargo-release 或额外自动化（保持简单）。
- **D-10:** Release 前在 CHANGELOG.md 中记录变更（cargo-dist 可自动生成 release notes from git log，可选启用）。

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 项目配置
- `Cargo.toml` — 当前包配置、依赖、二进制名 (`dm-installer`)；Phase 4 需在此添加 `[workspace.metadata.dist]`
- `.github/workflows/update-versions.yml` — 现有 CI 工作流参考；新建 `release.yml` 不应冲突
- `versions.txt` — DM8 下载 URL 索引，由 update-versions.sh 维护，Phase 4 不修改

### 发布工具
- `cargo-dist` 官方文档 (https://opensource.axo.dev/cargo-dist/) — `dist init`、`dist build`、`dist plan` 命令；GitHub Actions 集成
- `cross` GitHub (https://github.com/cross-rs/cross) — 交叉编译使用方式

### 需求追踪
- `.planning/REQUIREMENTS.md` §PLAT-01..PLAT-04 — 四个平台需求定义
- `.planning/ROADMAP.md` §Phase 4 — 成功标准（5条）

### 代码审查问题（需在 Phase 4 前或并行解决）
- `.planning/phases/03-cluster/03-REVIEW.md` — 5个 Critical 问题（sftp CREATE 标志、ISO 解压、tilde 展开、shell 注入、SSH TOFU 指纹）；这些是 cluster 功能的 bug，但影响 Phase 4 发布质量

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/cli.rs` — `Commands` enum 和 clap derive 模式；PLAT-04 `install windows` 子命令可按此模式添加
- `.github/workflows/update-versions.yml` — GitHub Actions 作业结构参考（checkout、commit、push 模式）
- `scripts/update-versions.sh` — 脚本放置惯例

### Established Patterns
- `reqwest` 已配置 `rustls-tls`（无 OpenSSL），三平台静态链接不受影响
- `Cargo.toml` `[profile.release]` 无特殊设置，cargo-dist 可直接使用
- 二进制名 `dm-installer`（`[[bin]] name = "dm-installer"`）是公开 API，Release asset 命名以此为基础

### Integration Points
- `Cargo.toml` → 需增加 `[workspace.metadata.dist]` 或创建 `dist-workspace.toml`
- `.github/workflows/` → 新建 `release.yml`（由 `cargo dist init` 生成）
- `README.md` → 安装说明需更新，指向 GitHub Releases 的下载链接

</code_context>

<specifics>
## Specific Ideas

- `cargo-dist` 的 `dist init` 交互式配置是起点，不要手写 release CI（会引入维护负担）
- 现有 `install.sh` 是 Phase 1 DM 安装脚本，必须保留；cargo-dist 生成的 bootstrap 是 `dm-installer` 工具安装脚本，两者功能不同，不要合并
- PLAT-04 Windows 目标机：优先让 x86_64-pc-windows-msvc 能编译（无 `todo!()` panic 在正常路径），实际 `setup.exe` 调用可用 `unimplemented!` 包裹并在文档注释中注明

</specifics>

<deferred>
## Deferred Ideas

- **Phase 3 代码审查 5 个 Critical 问题**：sftp CREATE 标志、ISO 解压缺失、`~` 不展开、shell 注入、TOFU 指纹——这些应在正式发布前修复，但属于 Phase 3 范围的 bug fix，不是 Phase 4 的核心功能。建议在 Phase 4 计划中作为 Wave 0 前置修复任务，或独立 gap-closure phase。
- **多 standby 节点**（`v2` 需求）——不在 v1 范围
- **自动下载 DM8 安装包**（DOWN-01）——Phase 1 部分实现，完整化留 v2
- **cargo-release 自动版本号管理**——当前手动更新版本号已够用

</deferred>

---

*Phase: 4-发布流水线*
*Context gathered: 2026-06-13*
