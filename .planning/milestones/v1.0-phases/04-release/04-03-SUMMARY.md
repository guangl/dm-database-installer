---
phase: "04-release"
plan: "03"
subsystem: "documentation-release"
tags:
  - documentation
  - release
  - changelog

dependency_graph:
  requires:
    - "04-02: cargo-dist 配置 + release.yml"
  provides:
    - "README.md: 安装指引 + 平台支持矩阵 + PLAT-04 限制说明"
    - "CHANGELOG.md: v1.0.0 发布说明（Keep a Changelog 格式）"
  affects: []

key_files:
  modified:
    - path: "README.md"
      changes: "完整重写：特性列表、双路安装指引（install.sh + dm-installer）、平台支持矩阵、PLAT-04 限制说明"
    - path: "CHANGELOG.md"
      changes: "新增 v1.0.0 段落，覆盖 Phase 1-4 全部新增功能、修复项和平台信息"

decisions:
  - "CHANGELOG 版本号从 v0.1.0 更新为 v1.0.0（与实际发布 tag 一致）"
  - "README 区分两个安装路径：install.sh（Phase 1，DM 数据库）vs dm-installer（Phase 2-4，CLI 工具）"
  - "新增 macOS 支持（cargo-zigbuild + glibc 2.23 兼容层）"
  - "新增 mdBook GitHub Pages 文档站"

metrics:
  completed_date: "2026-06-14"
  tasks_completed: 2
  files_changed: 2
---

# Phase 04 Plan 03: 文档完善 + 发布说明

**一句话：** README 提供双路安装指引（curl|bash + dm-installer），CHANGELOG 记录 v1.0.0 全部变更，发布前文档层就绪。

## 任务完成状态

| Task | 名称 | 状态 |
|------|------|------|
| 1 | README.md 安装段落 + 平台矩阵 + 限制说明 | 完成 |
| 2 | CHANGELOG.md v1.0.0 发布记录 | 完成 |
| 3 | 人工 checkpoint（发布验证） | 完成（v1.0.0 已 tag 并发布） |

## README.md 覆盖内容

- 特性列表（单机安装、SSH 远程、主备集群、断点续传、配置驱动）
- **方式一**：`curl | bash install.sh` 直接安装 DM8 数据库（Phase 1 产出）
- **方式二**：`curl | sh dm-database-installer-installer.sh` 安装 dm-installer 工具（cargo-dist 生成）
- 平台支持矩阵：Linux x86_64 / aarch64 / macOS x86_64 / Apple Silicon / Windows x86_64
- PLAT-04 限制说明：`install-windows` 是 placeholder，setup.exe 集成留 spike

## CHANGELOG.md v1.0.0 内容

新增：
- install.sh Phase 1 功能（多架构、密码生成、systemd 注册）
- dm-installer Phase 2 功能（TOML 配置、SSH 远程、断点续传）
- 主备集群 Phase 3 功能（批量 SSH、INI 配置分发）
- Phase 4 发布流水线（cargo-dist、zigbuild、macOS 支持）

修复：
- Kylin V10 SP1 识别修复
- SSH 重试机制
- unzip 替代 -q xml 方案
- glibc 要求降至 2.23

## 发布状态

git tag v1.0.0 已推送，GitHub Releases 已发布含三平台二进制的 Release。

## Self-Check: PASSED

- README.md 含 install.sh 和 dm-installer 两路安装指引
- README.md 含平台支持矩阵（Linux/macOS/Windows）
- CHANGELOG.md 存在且含 v1.0.0 段落
- v1.0.0 tag 已推送，Release CI 成功
