# Requirements: 达梦数据库安装器 (dm-database-installer)

**Defined:** 2026-06-14
**Milestone:** v1.1 集群扩展
**Core Value:** 开发者一行命令搞定本地达梦环境，DBA 用配置文件完成生产集群部署——两类用户都不需要手动操作达梦原生安装程序。

## v1.1 Requirements

### RWS 读写分离集群

- [ ] **RWS-01**: 用户可执行 `dm-installer install rws` 完成读写分离集群完整部署（主库 + 只读备库，端到端）
- [ ] **RWS-02**: 部署完成后备节点自动通过 SQL 配置为只读模式（READ_ONLY 标志）

### 状态查询

- [ ] **STAT-01**: 用户可执行 `dm-installer status` 查询本地 DM 实例进程状态与端口监听
- [ ] **STAT-02**: status 命令读取 config.toml 节点列表，通过 SSH 查询所有远程节点状态
- [ ] **STAT-03**: 状态输出包含每个节点的进程状态、端口监听、数据库角色（PRIMARY/STANDBY/OPEN），格式为对齐表格

### DSC 共享存储集群

- [ ] **DSC-01**: 用户可执行 `dm-installer install dsc` 完成 DSC 共享存储集群完整部署
- [ ] **DSC-02**: 安装流程在所有节点自动调用 dmasmtool 初始化 ASM 磁盘组
- [ ] **DSC-03**: 第一节点执行 dminit（路径指向共享存储），其他节点直接挂载启动

## Future Requirements

### DPC 集群

- **DPC-01**: TOML 配置文件驱动的 DPC 集群安装（MP/BP/SP 三角色）

### Windows 完整支持

- **PLAT-04**: `dm-installer install-windows` 完整实现（setup.exe /q /XML 集成）

### 其他

- **DRY-01**: `--dry-run` 模式（打印执行计划而不实际执行）

## Out of Scope

| Feature | Reason |
|---------|--------|
| DMProxy 安装与配置 | DMProxy 是客户端/中间层决策，由用户自行配置；工具只负责数据库节点 |
| 容器/K8s 部署 | 不在初版范围，后续可扩展 |
| 多版本支持 | 官网只提供一个版本，固定最新版 |
| 达梦数据库升级/迁移 | 只负责全新安装 |
| 图形界面 (GUI) | 纯 CLI 工具 |
| dmwatcher/dmmonitor（DSC）| DSC 共享存储集群不需要守护进程，节点直接访问共享数据 |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| RWS-01 | Phase 5 | Pending |
| RWS-02 | Phase 5 | Pending |
| STAT-01 | Phase 6 | Pending |
| STAT-02 | Phase 6 | Pending |
| STAT-03 | Phase 6 | Pending |
| DSC-01 | Phase 7 | Pending |
| DSC-02 | Phase 7 | Pending |
| DSC-03 | Phase 7 | Pending |

**Coverage:**
- v1.1 requirements: 8 total
- Mapped to phases: 8 (Phase 5: 2, Phase 6: 3, Phase 7: 3)
- Unmapped: 0 ✓

---
*Requirements defined: 2026-06-14*
*Last updated: 2026-06-14 — traceability mapped to v1.1 roadmap phases*
