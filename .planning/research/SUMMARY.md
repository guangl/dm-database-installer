# Research Summary: 达梦数据库安装器 (dm-database-installer)

## Executive Summary

这是一个面向两类用户的 Rust CLI 安装器：开发者需要一行 `curl | sh` 在本地快速拉起达梦数据库；DBA/运维需要通过 TOML 声明式配置文件完成单机或多种集群拓扑的自动部署。对标工具是 TiUP（TiDB）和 OBD（OceanBase），核心差异在于达梦没有这类开源工具。

Rust 生态对此需求支持成熟：`clap` 处理子命令、`russh` 做无 C 依赖的 SSH 客户端（避免 libssh2 跨编译问题）、`reqwest` 流式下载、`serde+toml` 配置解析，`cargo-dist` 管理多平台二进制发布，所有关键依赖已在 crates.io 验证。

推荐"配置驱动 + 阶段流水线"架构：TOML 文件描述集群拓扑（standalone/primary_standby/DSC/DPC），`build_plan()` 将配置翻译为有序 `Vec<Phase>`，Phase 内节点任务 `tokio::join_all` 并行执行。`Executor` trait 封装本地与远程 SSH 执行差异，是测试和扩展的关键接缝。

## Recommended Stack

| Crate | Version | Purpose |
|-------|---------|---------|
| `clap` | 4.5.x | CLI 子命令解析，derive 模式 |
| `tokio` | 1.44.x | Async runtime，集群节点并行操作 |
| `serde` + `toml` | 1.0.x / 0.8.x | TOML 配置解析 |
| `russh` | 0.61.2 | 纯 Rust SSH 客户端（无 C 依赖，跨编译友好）|
| `russh-sftp` | 2.3.0 | SFTP 文件推送（安装包/配置文件）|
| `reqwest` | 0.13.x | HTTP 下载，`rustls-tls` feature（无 OpenSSL）|
| `thiserror` | 2.0.18 | 模块级类型化错误 |
| `anyhow` | 1.0.102 | 应用层错误传播 |
| `indicatif` | 0.17.x | 进度条 |
| `cargo-dist` | latest | `curl \| sh` 发布基础设施 |
| `cross` | latest | 交叉编译（Linux ARM + Windows）|

**关键决策：**
- 用 `russh` 而非 `ssh2`（后者是 C 绑定，跨编译困难）
- `reqwest` 必须用 `rustls-tls` feature，避免 OpenSSL 依赖
- `cargo-dist` 生成 `install.sh` + 校验和 + GitHub Actions 发布流水线

## Table Stakes Features

- `curl | sh` 一行命令安装单机（HTTPS + SHA-256 校验）
- TOML 配置文件驱动安装（4 种拓扑）
- dminit 不可修改参数（PAGE_SIZE/CHARSET/CASE_SENSITIVE/EXTENT_SIZE）的安装前确认
- dmdba 用户创建 + 权限设置（禁止 root 直接运行 DM 实例）
- systemd 服务注册（`dm_service_installer.sh` + `systemctl enable`）
- 安装包本地路径指定（官网无公开直链，自动下载为次要路径）
- SSH 多节点并行操作（集群模式）
- 配置文件生成（`dm.ini`, `dmmal.ini`, `dmarch.ini`, `dmwatcher.ini`, `dmdcr_cfg.ini`）

## Critical Pitfalls

### P1: dminit 不可修改参数（最高风险）
`PAGE_SIZE` / `EXTENT_SIZE` / `CASE_SENSITIVE` / `CHARSET` 设定后无法更改。安装器必须在 dminit 执行前显式展示并要求确认，不能用隐含默认值。

### P2: 达梦官网无公开直链
`eco.dameng.com` 下载需登录，无匿名 API。"自动下载"功能需要提前 spike 验证，**主路径应为用户提供本地安装包路径**，自动下载为增强功能。

### P3: SSH sudo TTY 问题
RHEL/CentOS 默认 `requiretty`，SSH exec channel 无 TTY，`sudo` 会失败。预检阶段必须验证 `sudo -n true`，否则集群安装中途静默失败。

### P4: 双阶段权限模型
达梦不允许 root 用户运行实例，但 post-install 脚本必须 root 执行。安装器需处理：dmdba 用户创建 → 以 dmdba 身份 init → root 执行 `root_installer.sh` 的完整流程。

### P5: 集群中断后 DCR 磁盘脏数据
DSC 集群安装中断后，DCR 磁盘写入脏状态，导致无法重新安装。必须实现状态文件 + `dm-installer cluster clean` 子命令输出明确的 `dmasmcmd` 清理命令。

### P6: 幂等性
用户可能重复运行安装器。每个阶段必须检测当前状态（实例是否已存在、服务是否已注册）并正确处理，而非直接报错或覆盖。

## Architecture Overview

```
CLI (clap)
  └── Command dispatch
        ├── install --standalone         → StandaloneTopology
        ├── install --config config.toml → parse → TopologyConfig enum
        │     ├── Standalone
        │     ├── PrimaryStandby
        │     ├── DSC
        │     └── DPC
        └── cluster clean

TopologyConfig → build_plan() → Vec<Phase>
  Phase 1: Preflight checks (parallel per node)
  Phase 2: Download + verify package
  Phase 3: Push binary to remote nodes (SFTP)
  Phase 4: Install DM binary (invoke DMInstall.bin -q <xml>)
  Phase 5: Init instances (dminit)
  Phase 6: Generate config files (dm.ini, dmmal.ini, ...)
  Phase 7: Start services (systemd)
  Phase 8: Verify connectivity

Executor trait
  ├── LocalExecutor  (单机模式)
  └── SshExecutor    (集群模式，russh + russh-sftp)
```

## Recommended Phase Order

| Phase | Focus | Notes |
|-------|-------|-------|
| 1 | CLI 骨架 + 单机安装 | 验证 dminit/systemd 完整链路 |
| 2 | 发布基础设施 | cargo-dist, cross，`curl \| sh` 真正可用 |
| 3 | SSH 框架 + 主备集群 | russh Executor，2 节点主备 |
| 4 | 开发者体验 | `--dry-run`, `validate`, 幂等重试, `cluster clean` |
| 5 | DSC/DPC 集群 | 最复杂，依赖 Phase 3 成熟 |

## Research Gaps (需要 Spike)

- **达梦官网下载 API**：是否存在无认证直链（影响自动下载可行性）
- **Windows DM 安装包标志位**：`/q /XML` 等价参数需对照官方文档验证
- **russh 持久连接池**：大规模集群（10+ 节点）的连接复用需代码验证
- **DPC 角色差异化配置**：MP/BP/SP 各角色的 `dmarch.ini` 字段差异需官方文档核实
- **dm.key 集群授权格式**：影响 DSC/DPC 部署的授权检测逻辑

---
*Synthesized: 2026-06-12 from STACK.md, FEATURES.md, ARCHITECTURE.md, PITFALLS.md*
