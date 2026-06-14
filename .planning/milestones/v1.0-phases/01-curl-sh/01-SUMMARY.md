---
phase: "01-curl-sh"
plan: "shell"
subsystem: "install-script"
tags:
  - bash
  - install
  - curl-sh

dependency_graph:
  requires: []
  provides:
    - "install.sh: curl|bash 单命令完整安装达梦数据库"
  affects:
    - "04-03: README.md 引用 install.sh 安装路径"

key_files:
  modified:
    - path: "install.sh"
      changes: "完整实现：多架构检测、OS 回退逻辑、密码生成、systemd 服务注册"

decisions:
  - "纯 shell 实现，无外部依赖（符合 CLAUDE.md Phase 1 = 纯 shell 约束）"
  - "从 versions.txt 精确匹配下载链接，含 OS 回退逻辑"
  - "自动生成满足达梦密码策略的随机密码"
  - "支持架构：x86_64 / aarch64 / loongarch64 / mips64el / sw_64"
  - "支持 CPU 型号识别：Hygon、飞腾、鲲鹏等国产 CPU"

metrics:
  completed_date: "2026-06-14"
  files_changed: 1
---

# Phase 01: curl|sh 单机安装

**一句话：** 纯 shell 脚本实现 `curl | bash` 一行安装 DM8 数据库，支持 5 种架构、自动生成密码、systemd 服务注册。

## 功能覆盖

| 需求 | 描述 | 状态 |
|------|------|------|
| INST-01 | curl\|sh 一行命令安装 | 完成 |
| INST-03 | 不可修改参数展示（通过生成随机密码隐式处理） | 完成 |
| INST-04 | systemd 服务注册 + 开机自启 | 完成 |
| DOWN-01 | 从官方渠道自动下载安装包 | 完成 |
| DOWN-02 | SHA-256 校验和验证 | 完成 |
| QUAL-02 | 幂等性：已有实例则提示 | 完成 |

## 实现要点

- `versions.txt` 驱动的下载链接匹配（精确 OS + 架构 → 回退链）
- 自动生成满足达梦密码策略的随机 SYSDBA / SYSAUDITOR 密码
- 安装完成后打印凭证卡片和连接命令
- 注册 `DmAPService` 和 `DmService<INSTANCE>.service` 两个 systemd 服务

## Self-Check: PASSED

- install.sh 存在且可执行
- commit 46eb37e 记录了实现
- CHANGELOG.md v1.0.0 新增段落覆盖 Phase 1 功能
