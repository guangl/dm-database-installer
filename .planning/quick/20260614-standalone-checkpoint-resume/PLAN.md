---
slug: standalone-checkpoint-resume
date: 2026-06-14
status: in-progress
---

# standalone 安装中断续传

## Goal
安装被中断后（Ctrl+C / 网络断 / 重启），再次运行同样命令自动从断点继续，不重跑已完成的耗时步骤。

## Changes

### 1. Cargo.toml
- 添加 `serde_json = "1"`

### 2. src/standalone/checkpoint.rs (新建)
- `Checkpoint` 结构体：install_path / sysdba_pwd / sysauditor_pwd / installed
- `load(install_path)` — 从 CWD/dm_installer_checkpoint.json 加载，install_path 不匹配则忽略
- `Checkpoint::save()` — 写入 CWD/dm_installer_checkpoint.json
- `Checkpoint::remove()` — 成功后删除

### 3. src/standalone/mod.rs
- 添加 `pub mod checkpoint;` + `use std::path::Path;`
- run() 开头加载 checkpoint，恢复或新生成密码，立即保存 checkpoint
- step 5（silent_install）：`{install_path}/bin/dminit` 存在 OR `cp.installed` → 跳过
- step 6（dminit）：`{data_path}/dm.ini` 存在 → 跳过
- 成功后删除 checkpoint
- 移除旧 `check_idempotent_early_exit`（用路径检测替代，且原实现路径有误）

## Skip Logic
| 检测方式 | 含义 |
|---------|------|
| `cp.installed == true` | checkpoint 记录 silent_install 已完成 |
| `{install_path}/bin/dminit` 存在 | 无 checkpoint 时的回退检测（用户要求） |
| `{data_path}/dm.ini` 存在 | dminit 已运行过 |
