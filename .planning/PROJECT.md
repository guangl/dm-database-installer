# 达梦数据库安装器 (dm-database-installer)

## What This Is

一个 Rust CLI 工具（+ 纯 shell 脚本），自动化安装达梦数据库（DM8）。面向开发者，提供 `curl | bash install.sh` 一行命令快速拉起单机环境；面向 DBA/运维，通过 TOML 配置文件精细控制单机、主备集群的完整部署流程，支持 SSH 远程操作多节点。

## Core Value

开发者一行命令搞定本地达梦环境，DBA 用配置文件完成生产集群部署——两类用户都不需要手动操作达梦原生安装程序。

## Current Milestone: v1.1 集群扩展

**Goal:** 补全 RWS 读写分离集群端到端可用、DSC 共享存储集群完整实现（含 ASM 初始化）、status 命令查询所有节点运行状态。

**Target features:**
- RWS 集群完整实现（补全 run_read_routing_phase，dm-installer install rws 端到端可走通）
- dm-installer status 命令（本地 + SSH 远程节点状态：进程/端口/数据库角色）
- DSC 共享存储集群部署（dmasmtool ASM 初始化 + 共享存储上的 dminit + 多节点启动）

## Requirements

### Validated

- ✓ `curl | bash` 一行命令安装 DM8（install.sh，五架构支持）— v1.0
- ✓ TOML 配置文件驱动的单机安装（自定义端口、路径、实例参数等）— v1.0
- ✓ TOML 配置文件驱动的主备集群安装（SSH 远程，INI 自动分发）— v1.0
- ✓ 集群有序启动（主节点健康检查后再启备节点）— v1.0
- ✓ SSH 预检查（sudo 免密、端口、磁盘空间）— v1.0
- ✓ SHA-256 校验和验证 — v1.0
- ✓ 安装中断续传（checkpoint）— v1.0
- ✓ `dm-installer validate` 验证配置合法性 — v1.0
- ✓ 多平台发布流水线（Linux x86/aarch64、macOS、Windows）— v1.0
- ✓ PLAT-04 placeholder CLI（install-windows，实际集成留 spike）— v1.0

### Active

- [x] RWS 读写分离集群完整实现（v1.1）— 已在 Phase 05 验证
- [ ] `dm-installer status` 运行状态查询（本地 + SSH 远程，v1.1）
- [ ] TOML 配置文件驱动的 DSC 集群安装（多节点共享存储，含 ASM 初始化，v1.1）
- [ ] TOML 配置文件驱动的 DPC 集群安装（MP/BP/SP 三角色）
- [ ] PLAT-04 完整实现：setup.exe /q /XML 集成（Windows 目标机）
- [ ] `--dry-run` 模式

### Out of Scope

- 容器/K8s 部署 — 不在初版范围，后续可扩展
- 多版本支持 — 官网只提供一个版本，固定最新版
- 达梦数据库升级/迁移 — 只负责全新安装
- 图形界面 (GUI) — 纯 CLI 工具

## Context

- v1.0.0 已发布（2026-06-14），GitHub Releases 含 Linux x86_64/aarch64、macOS x86_64/Apple Silicon、Windows x86_64 五平台二进制
- 主备集群功能已通过自动化测试验证；双节点真实部署待人工验证
- install.sh 已支持 x86_64 / aarch64 / loongarch64 / mips64el / sw_64 五种架构
- 代码规模：Rust 约 3,000+ LOC（src/），shell 约 400 LOC（install.sh）
- 技术栈：Rust + tokio + russh + reqwest（rustls-tls）+ clap + serde/toml

## Constraints

- **Phase 1 实现**: 纯 bash/sh 脚本（install.sh）— 无外部依赖，curl|sh 友好
- **Phase 2+ 实现**: Rust — 性能和跨平台部署需求
- **Config Format**: TOML — Rust 生态首选，层级嵌套自然
- **Version Strategy**: 固定单版本 — 官网最新，无需版本矩阵
- **Distribution (单行命令)**: `curl | sh` 风格 — 开发者最低摩擦体验
- **Cluster Execution**: 单点 SSH 远程推送 — 用户无需在每个节点手动操作
- **Platforms**: Linux (x86/ARM/国产架构) + macOS + Windows — 三大平台

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust CLI | 跨平台发布，单二进制，无运行时依赖 | ✓ 成功：cargo-dist 三平台构建 |
| TOML 配置格式 | Rust 生态首选，`serde` 生态成熟，层级嵌套直观 | ✓ 成功：DBA 验证配置体验良好 |
| SSH 远程操作集群节点 | 用户只需在一台控制机执行，符合运维习惯 | ✓ 成功：russh 无 C 依赖，跨编译友好 |
| `curl \| sh` 单行命令 | 开发者最低摩擦安装体验 | ✓ 成功：install.sh v1.0 上线 |
| russh 替代 ssh2 | 无 C FFI，跨编译友好；ssh2 维护停滞 | ✓ 成功：aarch64 交叉编译无问题 |
| reqwest rustls-tls | 避免 OpenSSL 依赖，跨编译友好 | ✓ 成功：Linux/macOS/Windows 统一 |
| cargo-dist + zigbuild | zigbuild 控制 glibc 版本（≥2.23）；dist 自动生成 CI 和 installer 脚本 | ✓ 成功：glibc 2.23 兼容 Ubuntu 16.04+ |
| Windows target: msvc 非 gnu | ring + mio 对 windows-gnu 有兼容性问题 | ✓ 成功：windows-msvc 在 windows-2022 runner 原生构建通过 |
| PLAT-04 placeholder (eprintln+exit) | 用户体验比 todo!() panic 更友好；给未来 spike 留位置 | ✓ 验证：CLI 入口存在，限制说明清晰 |

## Evolution

**After each phase transition:**
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone:**
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-06-14 — Phase 05 (rws) complete: checkpoint 断点续传 + run_read_routing_phase 集成*
