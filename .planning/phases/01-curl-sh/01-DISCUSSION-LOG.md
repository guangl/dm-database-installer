# Phase 1: curl|sh 单机安装 - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-06-12
**Phase:** 1-curl|sh 单机安装
**Mode:** --auto (fully autonomous, no user interaction)
**Areas discussed:** 安装包获取策略, CLI 入口结构, INST-03 确认流程, curl|sh 默认参数, 幂等性检测

---

## 安装包获取策略 (DOWN-01 / DOWN-02)

| Option | Description | Selected |
|--------|-------------|----------|
| `--package` 本地路径为主 | Phase 1 主路径使用本地包，下载占位；不被 URL 可行性阻塞 | ✓ |
| 实现自动下载 | 从达梦官网直接下载；STATE.md P2 blocker 指出无公开直链 | |

**Auto-selected:** `--package` 本地路径为主（recommended default per STATE.md P2 blocker）
**Rationale:** STATE.md 明确："DOWN-01 自动下载需 spike 验证可行性；主路径建议先支持本地包路径"

---

## CLI 入口结构

| Option | Description | Selected |
|--------|-------------|----------|
| install + validate 子命令 | `dm-installer install [--package] [--defaults]` + `dm-installer validate --config` | ✓ |
| 平铺 flags | 默认行为就是安装，所有功能通过 flag 区分 | |

**Auto-selected:** install + validate 子命令（recommended default）
**Rationale:** QUAL-03 明确需要 `validate` 子命令；clap subcommand 模式语义清晰，Phase 2 扩展 `--config` 到 install 自然

---

## INST-03 不可修改参数确认流程

| Option | Description | Selected |
|--------|-------------|----------|
| 交互确认 + --defaults 跳过 | 默认展示参数并等待 y/n；`--defaults/--yes` 跳过；curl|sh 脚本传 --defaults | ✓ |
| 始终交互 | 不提供跳过 flag；curl|sh 场景会阻塞 | |
| 始终跳过 | 不展示不可修改参数；违反 INST-03 需求 | |

**Auto-selected:** 交互确认 + --defaults 跳过（recommended — pipe 场景必须非阻塞）
**Rationale:** INST-03 要求展示确认，但 curl|sh 是管道执行必须无交互；两个需求都要满足

---

## curl|sh 默认参数

| Option | Description | Selected |
|--------|-------------|----------|
| DM 官方默认 | PAGE_SIZE=8, CHARSET=GB18030, CASE_SENSITIVE=Y, EXTENT_SIZE=16, 路径 /opt/dmdbms, 端口 5236 | ✓ |
| 自定义默认 | 偏离官方默认，可能导致与 DM 文档不一致 | |

**Auto-selected:** DM 官方默认值（recommended — 最低摩擦，与官方文档对齐）

---

## 幂等性检测 (QUAL-02)

| Option | Description | Selected |
|--------|-------------|----------|
| 检测 dm.ini 存在则提示退出 | exit(0) + 提示信息，不覆盖不崩溃 | ✓ |
| 检测到已有实例则报错 | exit(1)，用户需手动清理 | |
| 强制覆盖 flag | `--force` 跳过检测；首版不需要 | |

**Auto-selected:** 检测 dm.ini 存在则提示退出（recommended — non-destructive default）

---

## Claude's Discretion

- 日志和进度展示：indicatif + console（用户未明确指定，Claude 选择）
- 错误处理分层：anyhow（顶层）+ thiserror（模块）
- systemd unit file 写入路径和服务名：`/etc/systemd/system/dmserver.service`

## Deferred Ideas

- DOWN-01 完整自动下载实现 — 需 spike 验证官网直链，Phase 1 后补
- Windows 服务注册 (INST-04 Windows) — Phase 4 跨平台发布时实现
- 断点续传 (DOWN-V2-01) — v2 需求
- `--dry-run` 模式 (OPS-V2-02) — v2 需求
