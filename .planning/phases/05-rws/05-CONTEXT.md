# Phase 5: RWS 读写分离集群 - Context

**Gathered:** 2026-06-14
**Status:** Ready for planning

<domain>
## Phase Boundary

补全 `run_read_routing_phase`，使 `dm-installer install rws` 端到端可走通。核心交付：

1. **轻量级集群 checkpoint**：在高代价 phases（preflight、install、primary_init、backup）完成后各自打点，standby_restore 之后记录一个整体标志，支持中断重跑跳过已完成步骤。
2. **`run_read_routing_phase`**：在 dmwatcher 启动后等待并验证 `read_only=true` 备节点达到 `MODE=STANDBY, STATUS=OPEN`（超时 120s）。不执行额外 SQL——dmwatcher 自动完成状态转换。

**不在本 phase 范围：**
- 修改 `run_verify_phase` 逻辑
- 调用 `configure_read_only_standby()`（`alter database open read only` 由 dmwatcher 自动执行）
- DMProxy 安装与配置

</domain>

<decisions>
## Implementation Decisions

### 断点恢复（Checkpoint）

- **D-01:** 实现轻量级 phase-level checkpoint，文件名 `dm_cluster_checkpoint.json`，存放在**当前工作目录**（和 rws.toml 同级）。
- **D-02:** checkpoint 颗粒度：以下各 phase 完成后单独记录：
  - `preflight_done`
  - `install_done`
  - `primary_init_done`
  - `backup_done`
  - `standby_restore_done`（备份传输完成后的整体标志，涵盖 standby_restore_phase 及之后全部步骤）
- **D-03:** standby_restore_done 之后的 phases（distribute、startup、watcher、monitor、sqllog、verify、read_routing）**不单独打点**。如果这些步骤中任一失败，重跑从 standby_restore 重试（即 standby_restore_done 未设置）。
- **D-04:** 部署全部完成后自动删除 checkpoint 文件。
- **D-05:** 数据结构类似 `src/standalone/checkpoint.rs`，用 JSON，但字段为各 phase 布尔标志。

### 只读备库开启时机

- **D-06:** `alter database open read only` **不需要**由安装器显式执行——dmwatcher 启动后自动将备节点状态从 MOUNT 转换为 OPEN。
- **D-07:** `run_read_routing_phase` 的职责是**等待 + 验证**，不执行 SQL。
- **D-08:** 备节点在 V$INSTANCE 的预期最终状态：`MODE$=STANDBY, STATUS$=OPEN`。
- **D-09:** 等待参数：超时 120 秒，间隔 5 秒轮询一次（最多 24 次重试）。

### run_read_routing_phase 实现

- **D-10:** 在 `src/cluster/phases.rs` 新增 `run_read_routing_phase` 函数。
- **D-11:** 函数签名：`pub async fn run_read_routing_phase(specific: &ClusterSpecificConfig, runners: &Runners, dminit: &DminitConfig) -> Result<()>`
- **D-12:** 逻辑：遍历 `specific.nodes`，找 `role == Standby && read_only == true` 的节点，对每个节点 poll `SELECT STATUS$,MODE$ FROM V$INSTANCE` 直到 `STATUS$=OPEN`，或超时后返回 Error。
- **D-13:** 在 `src/cluster/rws/mod.rs` 中，在 `run_verify_phase` **之后**调用（替换当前 TODO 注释）。

### Claude's Discretion

- checkpoint 文件的具体 JSON schema（字段命名、是否含时间戳）——与 standalone 保持一致即可
- 轮询 V$INSTANCE 时使用 `deploy::verify_node_role` 的变体还是新建一个专用的 `wait_for_standby_open` 函数——取决于代码复用度判断

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 现有集群 phases 结构
- `src/cluster/phases.rs` — 所有 run_*_phase 函数的实现，新函数须在此添加，命名/签名与现有模式一致
- `src/cluster/rws/mod.rs` — RWS 入口，run_read_routing_phase 须在 run_verify_phase 之后调用（替换 TODO:50）
- `src/cluster/deploy.rs:438-461` — `configure_read_only_standby()` 已实现但本 phase **不调用**；`verify_node_role`（`deploy.rs:395`）提供 V$INSTANCE 查询参考
- `src/cluster/deploy.rs:391-436` — `verify_node_role` 现有逻辑，poll 实现可参考其 SQL 模式

### Checkpoint 参考实现
- `src/standalone/checkpoint.rs` — standalone checkpoint 完整实现，集群 checkpoint 设计参考此模式（JSON 文件，各步骤布尔标志，完成后删除）

### 配置结构
- `src/config/cluster.rs:186-210` — `NodeConfig` 定义，`read_only: bool` 字段（默认 false）；`validate_rws()` 在 `281-293` 行
- `rws.toml` — 用户配置示例，已含 `read_only = true` 备节点示例

### 需求与验收标准
- `.planning/REQUIREMENTS.md` RWS-01, RWS-02
- `.planning/ROADMAP.md` Phase 5 Success Criteria（3条）

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `deploy::verify_node_role` (`deploy.rs:395`)：已封装 `SELECT STATUS$,MODE$ FROM V$INSTANCE` + disql 执行，poll 逻辑可基于此扩展（或提取 `wait_for_standby_open` 复用 SQL 构建方式）
- `phases::Runners` 类型别名 (`phases.rs:9`)：`Vec<(NodeConfig, Arc<dyn CommandRunner>)>`，run_read_routing_phase 参数类型与所有现有函数一致
- `standalone::Checkpoint` 实现（`src/standalone/checkpoint.rs`）：JSON checkpoint CRUD 模式的完整参考

### Established Patterns
- **Phase 函数签名**：`pub async fn run_xxx_phase(specific/runners/dminit) -> Result<()>`——新函数必须遵循此模式
- **Phases 里不做 SSH 连接**：SSH 连接在调用方（`rws/mod.rs`）完成，runners 传入
- **Tracing log 格式**：`tracing::info!("[cluster][N/M] ...")` ——新 phase 应使用 `[cluster][12/12]` 标记

### Integration Points
- `rws/mod.rs:50`：TODO 注释处直接调用 `phases::run_read_routing_phase(&specific, &runners, &dminit).await?`
- Checkpoint 需嵌入 `rws/mod.rs::run_with_runners` 的各 phase 调用之后
- Checkpoint 文件路径：当前工作目录，`dm_cluster_checkpoint.json`（和 config.toml/rws.toml 同级）

</code_context>

<specifics>
## Specific Ideas

- dmwatcher 启动后备节点自动转为 STATUS=OPEN，**不需要** `alter database open read only`——这是关键认知，避免在 run_read_routing_phase 里写 SQL
- 备节点预期状态：`MODE$=STANDBY, STATUS$=OPEN`（不是 MODE=PRIMARY）
- 超时 120s、间隔 5s（最多 24 次）——与现有 TCP 健康检查超时（60s）保持量级一致，给双倍余量
- checkpoint 文件在当前工作目录，完成后自动删除，用户可手动删除强制重跑

</specifics>

<deferred>
## Deferred Ideas

- `configure_read_only_standby()`（`deploy.rs:438`）目前无调用场景——未来若有需要手动触发只读的场景可用
- 备份传输后各 steps（distribute/startup/watcher 等）的细粒度 checkpoint——用户明确表示 standby_restore_done 后只记录一个整体标志

</deferred>

---

*Phase: 5-RWS 读写分离集群*
*Context gathered: 2026-06-14*
