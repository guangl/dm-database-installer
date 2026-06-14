# Roadmap: 达梦数据库安装器 (dm-database-installer)

## Milestones

- ✅ **v1.0 MVP** — Phases 1-4 (shipped 2026-06-14)
- 🚧 **v1.1 集群扩展** — Phases 5-7 (in progress)

## Phases

<details>
<summary>✅ v1.0 MVP (Phases 1-4) — SHIPPED 2026-06-14</summary>

- [x] Phase 1: curl|sh 单机安装 (1/1 plans) — completed 2026-06-14
- [x] Phase 2: TOML 配置驱动单机 (1/1 plans) — completed 2026-06-12
- [x] Phase 3: 主备集群 (3/3 plans) — completed 2026-06-12
- [x] Phase 4: 发布流水线 (3/3 plans) — completed 2026-06-14

Full details: `.planning/milestones/v1.0-ROADMAP.md`

</details>

### 🚧 v1.1 集群扩展 (In Progress)

**Milestone Goal:** 补全 RWS 读写分离集群端到端可用、DSC 共享存储集群完整实现（含 ASM 初始化）、status 命令查询所有节点运行状态。

- [ ] **Phase 5: RWS 读写分离集群** — 补全 run_read_routing_phase，使 dm-installer install rws 端到端可走通
- [ ] **Phase 6: status 命令** — 新增 dm-installer status 子命令，查询本地与所有远程节点状态
- [ ] **Phase 7: DSC 共享存储集群** — 完整实现 DSC 部署：ASM 初始化 + 共享存储 dminit + 多节点启动

## Phase Details

### Phase 5: RWS 读写分离集群
**Goal**: `dm-installer install rws` 端到端可走通，备库自动配置只读模式
**Depends on**: Phase 4 (v1.0)
**Requirements**: RWS-01, RWS-02
**Success Criteria** (what must be TRUE):
  1. 用户执行 `dm-installer install rws` 后主库与只读备库全部启动，无需手动操作
  2. 备节点启动后通过 SQL 自动设置 READ_ONLY 标志，客户端连接只读端口不可执行写操作
  3. 安装中断后重跑可从检查点恢复，不重复已完成步骤
**Plans**: TBD

### Phase 6: status 命令
**Goal**: `dm-installer status` 命令查询本地及所有远程节点的进程/端口/角色状态，输出对齐表格
**Depends on**: Phase 5
**Requirements**: STAT-01, STAT-02, STAT-03
**Success Criteria** (what must be TRUE):
  1. 用户执行 `dm-installer status` 后在终端看到本地 DM 实例的进程状态与端口监听
  2. 若当前目录存在 config.toml，命令自动通过 SSH 查询配置中所有远程节点状态
  3. 输出表格包含每节点的进程状态（running/stopped）、端口是否监听、数据库角色（PRIMARY/STANDBY/OPEN）
  4. 某节点 SSH 连接失败时，该行显示错误原因，其余节点正常输出
**Plans**: TBD

### Phase 7: DSC 共享存储集群
**Goal**: `dm-installer install dsc` 完成 DSC 共享存储集群完整部署，含 ASM 初始化
**Depends on**: Phase 6
**Requirements**: DSC-01, DSC-02, DSC-03
**Success Criteria** (what must be TRUE):
  1. 用户执行 `dm-installer install dsc` 后所有节点完成安装并连接至共享存储，无需手动操作
  2. 安装流程在每个节点上自动调用 dmasmtool 初始化 ASM 磁盘组，日志可见各节点 ASM 初始化结果
  3. 第一节点执行 dminit（路径指向共享存储），其余节点直接挂载共享存储并启动，不重复 dminit
  4. 所有节点启动后 V\$INSTANCE 角色查询返回预期值，集群可对外提供服务
**Plans**: TBD

## Progress

**Execution Order:** Phases execute in numeric order: 5 → 6 → 7

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. curl\|sh 单机安装 | v1.0 | 1/1 | Complete | 2026-06-14 |
| 2. TOML 配置驱动单机 | v1.0 | 1/1 | Complete | 2026-06-12 |
| 3. 主备集群 | v1.0 | 3/3 | Complete | 2026-06-12 |
| 4. 发布流水线 | v1.0 | 3/3 | Complete | 2026-06-14 |
| 5. RWS 读写分离集群 | v1.1 | 0/TBD | Not started | - |
| 6. status 命令 | v1.1 | 0/TBD | Not started | - |
| 7. DSC 共享存储集群 | v1.1 | 0/TBD | Not started | - |
