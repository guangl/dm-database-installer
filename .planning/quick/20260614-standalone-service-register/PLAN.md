---
slug: standalone-service-register
created: 2026-06-14
---

# 单机安装后注册并启动 DM 服务

## Goal
在 dminit 完成后，自动注册 systemd 服务并启动数据库，本地和 SSH 远程两条路径都要处理。

## Tasks
1. 新建 `src/standalone/service.rs`，实现本地服务注册与启动
2. 更新 `src/standalone/mod.rs`，在 dminit 之后调用 service 步骤
3. 更新 `src/standalone/remote.rs`，在 remote dminit 之后添加远程服务步骤
4. 更新步骤编号（从 6 步变为 7 步）
