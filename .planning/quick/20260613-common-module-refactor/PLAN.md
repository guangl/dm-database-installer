---
slug: common-module-refactor
created: 2026-06-13
status: in-progress
---

# Quick Task: 创建 src/common/ 共享模块

## Goal

将分散在 cluster/ 和 download/ 中的全局共享代码集中到 src/common/，使 standalone、cluster 各子类型均可复用，不再依赖彼此的内部路径。

## Scope

### 移动内容

| 来源 | 目标 | 说明 |
|------|------|------|
| `src/cluster/ssh.rs` | `src/common/ssh.rs` | SSH 连接、CommandRunner trait、MockRunner |
| `src/download/` | `src/common/download/` | HTTP 下载、版本解析、平台检测整体移入 |
| `src/download/detect.rs` (Platform) | `src/common/sysinfo.rs` | 平台/OS/arch 检测从 download 独立出来 |

### 配置拆分

| 来源 | 目标 | 说明 |
|------|------|------|
| `src/config/cluster.rs` SshCredentials | `src/config/ssh.rs` | 配置归配置，新建 ssh.rs，cluster.rs 改为引用 |

### 不动

- `src/config/cluster.rs` 其余结构体（NodeConfig、ClusterConfig 等）保持不变
- `src/cluster/` 内部业务逻辑（deploy、preflight、health、templates）原地不动
- `src/standalone/` 内部逻辑原地不动

## Steps

1. **新建 src/config/ssh.rs** — 从 cluster.rs 剪切 SshCredentials，在 cluster.rs 中 `use crate::config::ssh::SshCredentials`
2. **新建 src/common/mod.rs** — 声明子模块 pub mod ssh; pub mod sysinfo; pub mod download;
3. **新建 src/common/ssh.rs** — 从 cluster/ssh.rs 复制内容，修改 SshCredentials 引用路径
4. **新建 src/common/sysinfo.rs** — 从 download/detect.rs 提取 Platform struct 及检测逻辑
5. **移动 src/download/ → src/common/download/** — 整体移动，detect.rs 改为从 sysinfo re-export Platform
6. **更新 main.rs** — 去掉 `mod download;`，加 `mod common;`
7. **更新 cluster/mod.rs** — 去掉 `mod ssh;`，改引用 `crate::common::ssh`
8. **更新 cluster/primary_standby/mod.rs** — `use crate::cluster::ssh` → `use crate::common::ssh`
9. **删除 src/cluster/ssh.rs 和 src/download/**
10. **cargo build 验证编译通过**
11. **cargo test 验证测试通过**
12. **提交**
