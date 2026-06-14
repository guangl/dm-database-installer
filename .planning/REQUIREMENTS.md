# Requirements: 达梦数据库安装器 (dm-database-installer)

**Defined:** 2026-06-12
**Core Value:** 开发者一行命令搞定本地达梦环境，DBA 用配置文件完成生产集群部署——两类用户都不需要手动操作达梦原生安装程序。

## v1 Requirements

### 安装 (Installation)

- [x] **INST-01**: 用户可通过 `curl | sh` 一行命令安装单机达梦数据库，无需提前下载任何文件或编写配置
- [x] **INST-02**: 用户可通过 TOML 配置文件安装单机达梦，支持自定义端口、数据路径、页大小、字符集、大小写敏感等所有 dminit 参数
- [x] **INST-03**: 安装器在执行 dminit 前，明确展示 PAGE_SIZE/CHARSET/CASE_SENSITIVE/EXTENT_SIZE 这四个不可修改参数的当前值并要求用户确认
- [x] **INST-04**: 安装完成后自动将达梦实例注册为系统服务（Linux: systemd；Windows: Windows Service），并设置开机自启

### 集群 (Cluster)

- [x] **CLUS-01**: 用户可通过 TOML 配置文件部署主备集群，安装器通过 SSH 远程操作所有节点，自动生成并分发 dm.ini/dmmal.ini/dmarch.ini/dmwatcher.ini
- [x] **CLUS-02**: 集群部署时，主节点启动并确认健康后再启动备节点（有序启动，非并发）

### 下载 (Download)

- [x] **DOWN-01**: 安装器自动从达梦官方渠道下载对应平台（Linux x86/ARM、Windows）的安装包
- [x] **DOWN-02**: 下载完成后验证安装包 SHA-256 校验和，校验失败则拒绝继续安装

### 质量与安全 (Quality)

- [x] **QUAL-01**: 集群部署前执行 SSH 预检查：验证每个节点的 sudo 免密权限、目标端口可用性、磁盘剩余空间
- [x] **QUAL-02**: 安装器检测目标机器上的已有达梦实例，避免重复安装时覆盖或崩溃（幂等性）
- [x] **QUAL-03**: 用户可运行 `dm-installer validate --config config.toml` 仅验证配置文件合法性，不执行实际安装

### 平台 (Platform)

- [x] **PLAT-01**: 安装器可在 Linux x86_64 控制机上运行，并在 Linux x86_64 目标机上安装达梦
- [x] **PLAT-02**: 安装器可在 Linux aarch64 (ARM) 控制机上运行，并在 Linux aarch64 目标机上安装达梦
- [x] **PLAT-03**: 安装器可在 Windows 控制机上运行，并通过 SSH 在 Linux 目标节点上安装达梦
- [x] **PLAT-04**: 安装器支持在 Windows 目标机上安装达梦（Windows 数据库节点）— placeholder CLI 存在，setup.exe 集成留 spike

## v2 Requirements

### 集群 — 高级拓扑

- **CLUS-V2-01**: 用户可通过 TOML 配置文件部署 DSC 集群（分布式共享存储，需外部共享存储预先就绪）
- **CLUS-V2-02**: 安装失败后可运行 `dm-installer cluster clean` 清理 DSC 集群的 DCR 磁盘脏数据
- **CLUS-V2-03**: 用户可通过 TOML 配置文件部署 DPC 集群（MP/BP/SP 三角色差异化配置）

### 运维

- **OPS-V2-01**: 用户可运行 `dm-installer status` 查看已安装实例和集群各节点的运行状态
- **OPS-V2-02**: `--dry-run` 模式打印将要执行的所有操作而不实际执行

### 下载增强

- **DOWN-V2-01**: 支持断点续传，大安装包下载中断后可继续（不重新下载）

## Out of Scope

| Feature | Reason |
|---------|--------|
| 容器/K8s 部署 | 不在初版范围，后续可扩展 |
| 多版本达梦支持 | 官网只提供一个当前版本，无版本矩阵需求 |
| 达梦数据库升级/迁移 | 只负责全新安装，升级路径复杂度不同 |
| 图形界面 (GUI) | 纯 CLI 工具，GUI 是独立项目 |
| 达梦 DM8 之前版本 | 只支持当前最新 DM 版本 |
| PLAT-04 完整实现 | setup.exe /q /XML 集成留 v2 spike；v1 仅 placeholder CLI |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| INST-01 | Phase 1 | Complete |
| INST-02 | Phase 2 | Complete |
| INST-03 | Phase 1 | Complete |
| INST-04 | Phase 1 | Complete |
| CLUS-01 | Phase 3 | Complete |
| CLUS-02 | Phase 3 | Complete |
| DOWN-01 | Phase 1 | Complete |
| DOWN-02 | Phase 1 | Complete |
| QUAL-01 | Phase 3 | Complete |
| QUAL-02 | Phase 1 | Complete |
| QUAL-03 | Phase 2 | Complete |
| PLAT-01 | Phase 4 | Complete |
| PLAT-02 | Phase 4 | Complete |
| PLAT-03 | Phase 4 | Complete |
| PLAT-04 | Phase 4 | Complete (placeholder) |

**Coverage:**
- v1 requirements: 15 total
- Complete: 15 ✓
- Mapped to phases: 15 ✓
- Unmapped: 0

| Phase | Requirements | Count |
|-------|-------------|-------|
| Phase 1: curl\|sh 单机安装 | INST-01, INST-03, INST-04, DOWN-01, DOWN-02, QUAL-02 | 6 |
| Phase 2: TOML 配置驱动单机 | INST-02, QUAL-03 | 2 |
| Phase 3: 主备集群 | CLUS-01, CLUS-02, QUAL-01 | 3 |
| Phase 4: 发布流水线 | PLAT-01, PLAT-02, PLAT-03, PLAT-04 | 4 |

---
*Requirements defined: 2026-06-12*
*Last updated: 2026-06-14 — v1.0.0 milestone complete, all 15 requirements fulfilled*
