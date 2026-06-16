# Phase 3: 主备集群 - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-06-12
**Phase:** 3-主备集群
**Mode:** --auto (fully autonomous, no user interaction)
**Areas discussed:** TOML 集群配置 Schema, CLI 入口设计, SSH 认证策略, known_hosts 验证, 主节点健康判据

---

## TOML 集群配置 Schema

| Option | Description | Selected |
|--------|-------------|----------|
| `[[cluster.nodes]]` 数组 + `role` 字段 | 标准 TOML 数组，可扩展到多备节点，serde Vec 反序列化自然 | ✓ |
| `[cluster.primary]` + `[cluster.standby]` 分段 | 结构更明确，但硬编码双节点，不可扩展 | |

**Auto-selected:** `[[cluster.nodes]]` 数组 (recommended default)
**Notes:** 未来 1主N备扩展无需改变 schema，只需增加数组条目。

---

## CLI 入口设计

| Option | Description | Selected |
|--------|-------------|----------|
| 新增 `cluster deploy` 子命令 | 职责分离，与 install 行为模型不同，不混淆用户 | ✓ |
| 复用 `install --config` 检测 `[cluster]` 段 | 零新命令，但行为变化隐式，不直观 | |

**Auto-selected:** 新增 `cluster deploy` 子命令 (recommended default)
**Notes:** `install` 命令保持单机语义，`cluster deploy` 明确集群部署意图。

---

## SSH 认证策略

| Option | Description | Selected |
|--------|-------------|----------|
| 密钥为主，密码可选备用 | 生产安全标准，避免明文密码，russh 原生支持 | ✓ |
| 仅支持密码认证 | 实现简单但不适合生产 | |
| 仅支持密钥认证 | 最安全但灵活性低 | |

**Auto-selected:** 密钥为主，密码可选备用 (recommended default)
**Notes:** TOML 中 `identity_file` 必填或 `password` 必填，两者都缺则报错。

---

## known_hosts 验证策略

| Option | Description | Selected |
|--------|-------------|----------|
| TOFU（首次自动接受，会话内记录） | 安装器场景实用，减少配置摩擦，不持久写文件 | ✓ |
| 严格模式（必须已在 known_hosts） | 最安全但部署前需手动 ssh 每个节点 | |

**Auto-selected:** TOFU 策略 (recommended default)
**Notes:** 会话内记录不持久化，避免写 `~/.ssh/known_hosts` 权限问题；russh `ServerCheckHandler` 自定义实现。

---

## 主节点健康判据（CLUS-02）

| Option | Description | Selected |
|--------|-------------|----------|
| TCP 端口可达 + 超时重试 | 简单可靠，满足 CLUS-02 验收标准 | ✓ |
| SQL 查询验证 | 更准确，但需额外 SQL 客户端依赖 | |

**Auto-selected:** TCP 端口可达，60s 超时，3s 间隔 (recommended default)
**Notes:** 参数（超时/间隔）由 Claude 决定，默认值可在未来配置化。

---

## Claude's Discretion

- russh 使用 rustls backend（一致性）
- 主备并发推包，仅启动阶段有序
- 日志前缀格式 `[node:primary][N/M] 步骤名`
- 配置文件模板以 Rust `const` 或 `include_str!` 管理

## Deferred Ideas

- 多备节点（1主N备）— schema 已支持，执行逻辑 Phase v2
- DSC/DPC 集群拓扑 — v2 需求
- `--dry-run` 模式 — v2 需求
- 集群清理命令 `cluster clean` — v2 需求
- DOWN-01 自动下载 — 仍为 P2 风险，延续 Phase 2 决策
