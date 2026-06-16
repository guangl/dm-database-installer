# Phase 5: RWS 读写分离集群 - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-06-14
**Phase:** 5-RWS 读写分离集群
**Areas discussed:** 断点恢复, 只读备库开启时机, 读只备库验证策略

---

## 断点恢复（Checkpoint）

| Option | Description | Selected |
|--------|-------------|----------|
| 轻量级 phase checkpoint | 在每个 phase 完成后写 JSON 文件记录进度，重跑时跳过已完成步骤 | ✓ |
| 不实现，从成功标准移除此条 | 集群安装是幂等操作，不支持完美恢复 | |
| 记录到 CHANGELOG 并标注为待办 | 先完成 RWS 核心功能，checkpoint 作为待办项 | |

**用户的选择：** 轻量级 phase checkpoint

**文件位置：**
| Option | Description | Selected |
|--------|-------------|----------|
| 当前工作目录 | 和 rws.toml 同级，dm_cluster_checkpoint.json | ✓ |
| 系统临时目录 | /tmp/dm_cluster_checkpoint_{hash}.json | |

**checkpoint 颗粒度：**
| Option | Description | Selected |
|--------|-------------|----------|
| 每个 phase 都单独记录 | 最细粒度，最灵活 | |
| 备份传输之前每个操作都 checkpoint，之后只记录一个整体标志 | 备份传输后失败都需要从备份还原重跑 | ✓ |

**用户补充：** "备份传输之后只记录一个整体的标志了"——standby_restore_phase 完成后的步骤（distribute/startup/watcher/monitor/sqllog/verify/read_routing）不单独打点，失败了就从 standby_restore 重试。

**备份传输后是否各自 checkpoint：**
| Option | Description | Selected |
|--------|-------------|----------|
| 各 phase 单独 checkpoint（允许跳过已完成的） | 最大灵活性，但数据库状态可能不一致 | |
| 备份传输后只记录一个整体标志 | 失败了从 standby_restore 重试，更安全 | ✓ |

---

## 只读备库开启时机

| Option | Description | Selected |
|--------|-------------|----------|
| run_read_routing_phase 在 run_verify_phase 之前 | verify 可验证最终状态包括 STATUS=OPEN | |
| run_read_routing_phase 在 run_verify_phase 之后（TODO 当前位置） | 逻辑分离清晰，不影响 verify 逻辑 | |

**用户的关键澄清：** "不用执行什么东西，状态自然是只读的，但是备节点期望状态是 MODE=PRIMARY,STATUS=OPEN"

后续确认：
- `alter database open read only` 不需要显式执行——dmwatcher 启动后自动将备节点转换
- 备节点预期状态：`MODE$=STANDBY, STATUS$=OPEN`（用户补充：之前说 MODE=PRIMARY 是笔误）
- dmwatcher 启动后备节点不会立即达到 STATUS=OPEN，需要等待，要加重试循环
- 等待超时：120 秒（最多重试 N 次，间隔 5 秒）

---

## 读只备库验证策略

| Option | Description | Selected |
|--------|-------------|----------|
| 新建 run_read_routing_phase | 独立函数，负责 read_only 节点等待+验证 | ✓ |
| 修改 run_verify_phase 应对 RWS | 传入 ClusterSpecificConfig，内部判断是否 RWS | |

**位置：**
| Option | Description | Selected |
|--------|-------------|----------|
| run_verify_phase 之后（TODO 当前位置） | 逻辑分离清晰 | ✓ |
| run_watcher_phase 之后、verify 之前 | 更自然的序列，但需改 verify | |

**函数签名：**
| Option | Description | Selected |
|--------|-------------|----------|
| 接收 &ClusterSpecificConfig（推荐） | 通过 specific.nodes 找 read_only=true 节点 | ✓ |
| 只接收 Runners + DminitConfig | 调用方提前过滤 | |

---

## Claude's Discretion

- checkpoint 文件的具体 JSON schema（字段命名、是否含时间戳）——参考 standalone 实现
- 轮询 V$INSTANCE 时复用 `verify_node_role` 逻辑还是新建 `wait_for_standby_open`——基于代码复用度判断

## Deferred Ideas

- `configure_read_only_standby()`（`deploy.rs:438`）—— 本 phase 不使用，保留供未来场景
- 备份传输后细粒度 checkpoint（startup/watcher 等各步骤）—— 用户明确不在此 phase 实现
