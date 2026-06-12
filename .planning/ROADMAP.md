# Roadmap: 达梦数据库安装器 (dm-database-installer)

## Overview

从一行 `curl | sh` 可运行的单机安装器出发，逐步扩展到 TOML 配置驱动的精细控制、SSH 主备集群部署，最终通过 `cargo-dist` 实现多平台二进制的正式发布流水线，让开发者和 DBA 都能零摩擦使用。

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: curl|sh 单机安装** - 用户可用一行命令安装达梦，完整链路跑通（下载、校验、安装、注册服务）
- [ ] **Phase 2: TOML 配置驱动单机** - DBA 可通过配置文件精细控制单机安装的所有参数
- [ ] **Phase 3: 主备集群** - 用户可通过配置文件部署双节点主备集群，安装器自动 SSH 远程操作
- [ ] **Phase 4: 发布流水线** - 多平台二进制正式分发，`curl | sh` 对真实用户可用

## Phase Details

### Phase 1: curl|sh 单机安装
**Goal**: 用户可运行一行 `curl | sh` 命令完整安装达梦单机实例并注册为系统服务
**Mode:** mvp
**Depends on**: Nothing (first phase)
**Requirements**: INST-01, INST-03, INST-04, DOWN-01, DOWN-02, QUAL-02, QUAL-03
**Success Criteria** (what must be TRUE):
  1. 用户在 Linux x86_64 机器上运行一行命令后，达梦实例已启动并可接受连接
  2. 安装器在执行 dminit 前，显示 PAGE_SIZE/CHARSET/CASE_SENSITIVE/EXTENT_SIZE 的值并等待用户确认
  3. 安装完成后 `systemctl status dmserver` 显示服务已注册且开机自启
  4. 重复运行安装器时，检测到已有实例并提示而非覆盖崩溃（幂等性）
  5. 用户可运行 `dm-installer validate --config config.toml` 仅验证配置合法性而不执行安装
**Plans**: TBD

### Phase 2: TOML 配置驱动单机
**Goal**: DBA 可通过 TOML 配置文件自定义端口、数据路径、dminit 参数，完成单机安装
**Mode:** mvp
**Depends on**: Phase 1
**Requirements**: INST-02
**Success Criteria** (what must be TRUE):
  1. DBA 编写包含自定义端口、数据路径、页大小等参数的 TOML 文件后，安装器按配置完成安装
  2. TOML 文件中指定的所有 dminit 参数（PAGE_SIZE/CHARSET/CASE_SENSITIVE/EXTENT_SIZE/端口/路径）均生效
  3. 配置文件格式错误时，安装器给出清晰的错误信息指向具体字段，不执行安装
**Plans**: TBD

### Phase 3: 主备集群
**Goal**: 用户可通过一份 TOML 配置文件，在一台控制机上完成双节点主备集群的完整部署
**Mode:** mvp
**Depends on**: Phase 2
**Requirements**: CLUS-01, CLUS-02, QUAL-01
**Success Criteria** (what must be TRUE):
  1. 用户在控制机执行一条命令后，两个远程节点均完成达梦安装并建立主备复制关系
  2. 安装器自动生成并分发 dm.ini/dmmal.ini/dmarch.ini/dmwatcher.ini 到各节点
  3. 主节点启动并确认健康后才启动备节点（可从日志中观察到有序启动顺序）
  4. 集群部署开始前，安装器完成 SSH 预检查（sudo 免密、端口可用性、磁盘空间），任一检查失败则中止并报告
**Plans**: TBD

### Phase 4: 发布流水线
**Goal**: 多平台预编译二进制通过 GitHub Releases 分发，用户在任意支持平台可通过 `curl | sh` 实际安装
**Mode:** mvp
**Depends on**: Phase 3
**Requirements**: PLAT-01, PLAT-02, PLAT-03, PLAT-04
**Success Criteria** (what must be TRUE):
  1. Linux x86_64 用户可从公开 URL 运行 `curl | sh` 下载并执行安装器
  2. Linux aarch64 (ARM) 用户可从公开 URL 运行 `curl | sh` 下载并执行安装器
  3. Windows 用户可下载安装器并通过 SSH 在 Linux 目标节点上安装达梦
  4. Windows 目标机上可安装达梦实例
  5. GitHub Actions 在打 tag 时自动构建并发布所有平台的二进制到 GitHub Releases
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. curl\|sh 单机安装 | 0/TBD | Not started | - |
| 2. TOML 配置驱动单机 | 0/TBD | Not started | - |
| 3. 主备集群 | 0/TBD | Not started | - |
| 4. 发布流水线 | 0/TBD | Not started | - |
