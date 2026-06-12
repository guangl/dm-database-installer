# 达梦数据库安装器 (dm-database-installer)

## What This Is

一个 Rust CLI 工具，自动化安装达梦数据库。面向开发者，提供 `curl | sh` 一行命令快速拉起单机环境；面向 DBA/运维，通过 TOML 配置文件精细控制单机、主备、DSC 集群、DPC 集群的完整部署流程，支持 SSH 远程操作多节点。

## Core Value

开发者一行命令搞定本地达梦环境，DBA 用配置文件完成生产集群部署——两类用户都不需要手动操作达梦原生安装程序。

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] `curl | sh` 一行命令安装单机达梦（使用内置默认参数，无需配置文件）
- [ ] TOML 配置文件驱动的单机安装（支持自定义端口、路径、实例参数等）
- [ ] TOML 配置文件驱动的主备集群安装（主节点 + 备节点，SSH 远程部署）
- [ ] TOML 配置文件驱动的 DSC 集群安装（多节点，SSH 远程部署）
- [ ] TOML 配置文件驱动的 DPC 集群安装（多节点，SSH 远程部署）
- [ ] 自动从达梦官网下载对应平台的最新安装包
- [ ] 支持 Linux x86/ARM 物理机/VM
- [ ] 支持 Windows 环境
- [ ] 集群模式：单机执行，通过 SSH 远程推送并操作所有节点

### Out of Scope

- 容器/K8s 部署 — 不在初版范围，后续可扩展
- 多版本支持 — 官网只提供一个版本，固定最新版
- 达梦数据库升级/迁移 — 只负责全新安装
- 图形界面 (GUI) — 纯 CLI 工具

## Context

- 达梦数据库（DM）是国产关系型数据库，官方安装方式为交互式 `.bin` 安装程序，对开发者不友好
- 集群模式（主备/DSC/DPC）涉及多台机器协同配置，目前需要 DBA 手动在每台机器上操作
- 项目初始只有 Cargo 脚手架（`dm-database-installer` crate）
- Rust 实现，跨平台编译需覆盖 Linux x86/ARM 和 Windows

## Constraints

- **Tech Stack**: Rust — 已确定，性能和跨平台部署需求
- **Config Format**: TOML — Rust 生态首选，层级嵌套自然
- **Version Strategy**: 固定单版本 — 官网最新，无需版本矩阵
- **Distribution (单行命令)**: `curl | sh` 风格 — 开发者最低摩擦体验
- **Cluster Execution**: 单点 SSH 远程推送 — 用户无需在每个节点手动操作
- **Platforms**: Linux (x86/ARM) + Windows — 两类场景都要覆盖

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust CLI | 跨平台发布，单二进制，无运行时依赖 | — Pending |
| TOML 配置格式 | Rust 生态首选，`serde` 生态成熟，层级嵌套直观 | — Pending |
| SSH 远程操作集群节点 | 用户只需在一台控制机执行，符合运维习惯 | — Pending |
| `curl \| sh` 单行命令 | 开发者最低摩擦安装体验，无需预先下载 installer | — Pending |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd:complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-06-12 after initialization*
