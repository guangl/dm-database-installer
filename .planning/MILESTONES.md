# Milestones: 达梦数据库安装器 (dm-database-installer)

## v1.0 MVP

**Shipped:** 2026-06-14
**Phases:** 4 | **Plans:** 8 | **Requirements fulfilled:** 15/15

### Delivered

从零搭建达梦数据库自动化安装工具，覆盖开发者和 DBA 两类核心用户场景：
1. `curl | bash install.sh` 一行命令在 Linux 上安装 DM8（五架构支持）
2. TOML 配置文件驱动的精细单机安装（自定义端口、路径、实例参数）
3. SSH 主备集群部署（自动生成 INI、分发配置、有序启动）
4. 多平台发布流水线（Linux x86_64/aarch64、macOS、Windows 五平台二进制）

### Key Accomplishments

1. **install.sh 零摩擦安装**：纯 shell 脚本，无依赖，支持 x86_64/aarch64/loongarch64/mips64el/sw_64 五种架构和多种 Linux 发行版；自动生成随机 SYSDBA 密码并注册 systemd 服务
2. **TOML 配置驱动**：Rust CLI 读取配置文件，`dm-installer validate` 验证语义合法性，所有 dminit 参数均可配置
3. **SSH 主备集群**：控制机一条命令完成双节点安装，自动生成并分发 dm.ini/dmmal.ini/dmarch.ini/dmwatcher.ini，主节点健康检查后再启备节点
4. **SSH 安全加固**：修复 5 个 Critical 安全问题（SFTP CREATE 标志、shell 命令注入、tilde 路径展开、TOFU 指纹记录）
5. **断点续传**：安装中断后重跑自动从最近检查点恢复，不重复已完成步骤
6. **多平台发布**：cargo-dist + cargo-zigbuild，glibc ≥ 2.23 兼容，GitHub Actions 自动构建五平台二进制

### Stats

- Timeline: 2026-06-12 → 2026-06-14 (2 days)
- Git tag: v1.0.0
- Archives: `.planning/milestones/v1.0-ROADMAP.md`, `.planning/milestones/v1.0-REQUIREMENTS.md`

### Known Gaps at Close

- PLAT-04: `install-windows` 为 placeholder（eprintln + exit 1），setup.exe /q /XML 集成留 v2 spike
- 主备集群待人工验证（自动化测试全绿，真实双节点部署未在 CI 中验证）

---

## v1.1 集群扩展

**Shipped:** 2026-06-15
**Phases:** 3 (05-07) | **Plans:** 8 | **Requirements fulfilled:** 8/8

### Delivered

补全 RWS 读写分离集群端到端可用、实现 DSC 共享存储集群完整部署（含 ASM 初始化）、新增 status 命令查询所有节点运行状态：
1. `dm-installer install rws` 端到端可走通（备库 READ_ONLY + checkpoint 断点续传）
2. `dm-installer status` 并发 SSH 查询所有节点状态，输出五列对齐表格
3. `dm-installer install dsc` 完整编排 10 个部署阶段（DMCSS→DMASM→dminit→启动→验证）

### Key Accomplishments

1. **RWS 断点续传**：ClusterCheckpoint 5 字段 serde_json 持久化，5 个 gate 覆盖主要安装阶段，中断重跑跳过已完成步骤
2. **只读路由验证**：run_read_routing_phase 对 read_only 备库轮询 V$INSTANCE（STATUS$=OPEN MODE$=STANDBY），dmwatcher 自动转换只读模式（D-06）
3. **status 命令**：22 个单元测试，并发 SSH + 5s 超时，单节点失败降级不影响整体退出码
4. **DSC INI 模板**：SEQNO 按 node_index 动态生成，dminit.ini 路径以 + 开头（ASM 格式），5 个 DSC Pitfall 全部防御
5. **DSC 安全增强**：dminit.ini 执行后自动 `rm -f`（防明文密码遗留），validate_dsc 拒绝 Monitor 节点 + 强制 ≥2 个节点
6. **环境准备增强**：env_setup.rs 新增本地环境准备（dmdba 用户/SELinux/THP/timezone/limits），install.sh 新增端口/内存预检

### Stats

- Timeline: 2026-06-14 → 2026-06-15 (2 days)
- Commits: 95 since v1.0.0
- Files changed: 130+, +15,700/-1,146 lines
- Tests at close: 264 passed, 0 failed
- Git tag: v1.1.0
- Archives: `.planning/milestones/v1.1-ROADMAP.md`, `.planning/milestones/v1.1-REQUIREMENTS.md`

### Known Gaps at Close

- DSC 真实 2 节点 + 4 块共享块设备端到端部署待人工验证（有硬件环境时执行）
- DSC CLI 入口路径验证（bogus SSH 主机 dsc.toml 错误消息）待人工确认
- PLAT-04: `install-windows` 仍为 placeholder，carry forward 至下一里程碑
- RWS/主备集群真实多节点人工验证持续待办
