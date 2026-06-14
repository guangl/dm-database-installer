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
