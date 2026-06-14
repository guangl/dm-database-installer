# Phase 4: 发布流水线 - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-06-13
**Phase:** 4-发布流水线
**Mode:** --auto (fully autonomous, no user interaction)
**Areas discussed:** 发布工具选型, 目标平台矩阵, PLAT-04 scope, install.sh 集成方式

---

## 发布工具选型

| Option | Description | Selected |
|--------|-------------|----------|
| cargo-dist | 零配置 release CI 生成，CLAUDE.md 明确推荐 | ✓ |
| 手写 GitHub Actions CI | 完全自定义，维护负担高 | |
| goreleaser-style 工具 | Rust 生态支持弱 | |

**Auto-selected:** cargo-dist (recommended — per CLAUDE.md Tech Stack)
**Notes:** 项目 CLAUDE.md 和 Technology Stack 表格中明确列出 `cargo-dist` 作为 Phase 4 发布工具。

---

## 目标平台矩阵

| Option | Description | Selected |
|--------|-------------|----------|
| Linux x86_64 + aarch64 + Windows x86_64 | 覆盖 PLAT-01/02/03 | ✓ |
| 仅 Linux x86_64 | 最小范围 | |
| 全平台（含 macOS） | 超出需求范围 | |

**Auto-selected:** 三平台（x86_64-unknown-linux-gnu + aarch64-unknown-linux-gnu + x86_64-pc-windows-gnu）
**Notes:** 使用 cross 工具链；rustls-tls 已确保无 OpenSSL 依赖，静态链接安全。

---

## PLAT-04 Windows 目标机 DM 安装

| Option | Description | Selected |
|--------|-------------|----------|
| 纳入并作为 placeholder | CLI 结构就位，setup.exe 调用 todo!() | ✓ |
| 完整实现 | 需验证 DM Windows 安装包 URL，风险高 | |
| Defer 到 v2 | 不做，推迟整个需求 | |

**Auto-selected:** 纳入作为 placeholder
**Notes:** DM Windows 安装包（setup.exe /q /XML）需从 eco.dameng.com 单独验证可行性，不阻塞 PLAT-01/02/03。

---

## install.sh bootstrap 集成方式

| Option | Description | Selected |
|--------|-------------|----------|
| 保留现有 install.sh，cargo-dist bootstrap 独立 | 两个脚本功能不同，不合并 | ✓ |
| 将 cargo-dist bootstrap 合并进现有 install.sh | 容易混淆两种安装流程 | |
| 废弃现有 install.sh | 破坏 Phase 1 功能 | |

**Auto-selected:** 保留现有 install.sh（DM 安装器），cargo-dist 生成的 bootstrap 独立
**Notes:** 现有 install.sh 是 Phase 1 的达梦数据库安装脚本（下载 DM8 zip，安装 DM）；cargo-dist 生成的是 dm-installer 工具本身的安装脚本（让用户获得 dm-installer binary）。功能完全不同。

---

## Claude's Discretion

- 具体的 `dist-workspace.toml` 配置内容（installer-backends、artifacts 等）——由研究和计划阶段确认
- CHANGELOG.md 格式——使用 cargo-dist 自动从 git log 生成 release notes

## Deferred Ideas

- Phase 3 的 5 个 Critical 代码审查问题（sftp/shell注入等）——建议作为 Phase 4 Wave 0 前置修复或独立 gap-closure
- cargo-release 自动版本号管理——当前手动更新版本号已够用，v2 再考虑
- macOS 平台支持——不在 v1 需求中
