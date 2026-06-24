# 更新日志

## [Unreleased]

### 新增

- **三种备库类型**：`dw.toml` 备库节点新增 `sync_mode` 字段，支持 `realtime`（默认，参与自动切换）/ `sync`（同步备库）/ `async`（异步备库，需配套 `arch_timer_name`），对应 `dmwatcher.ini` 的 `DW_TYPE`（GLOBAL/LOCAL）与 `dmmonitor.ini` 仲裁列表自动收敛
- `dw.toml` 新增 `[monitor]` 配置段，`dmmonitor.ini` 的日志参数（路径/间隔/大小/空间上限）不再硬编码
- `[arch]` 段 `arch_space_limit` 改为可选：不填自动取磁盘总容量的 20%，探测失败时退回默认值 20480 MB（20GB）
- `dm_installer validate` 输出重做：彩色分栏展示完整生效配置，新增此前缺失的监视器配置展示

### 变更

- `dw.toml` 部分默认值调整：故障/连接相关超时统一改为 60 秒，`mon_log_space_limit` 默认改为 4096 MB
- `dm_installer init` 生成的模板新增"速览"区块，汇总常改字段（TOML 结构不变）

## [2.0.0] - 2026-06-23

### 新增

- **主备集群（DW）安装支持**：`dm_installer init dw` 生成配置模板，`dm_installer install` 按官方[数据守护搭建文档](https://eco.dameng.com/document/dm/zh-cn/pm/data-guard-construction.html)自动完成整套主备搭建（预检→环境准备→安装→`dminit`→备份还原同步备库数据→分发守护配置→Mount 模式启动并设置角色→注册并启动 `dmserver`/`dmwatcher`/`dmmonitor` 三个 systemd 服务→备份作业/SQL 日志/参数优化）
- **集群断点续传**：按节点维度的检查点覆盖每个独立步骤，中断后重跑自动跳过已完成部分
- `dw.toml` 的 `oguid` 可省略，默认当天 `YYYYMMDD`；新增 `[nodes.backup]`（仅 primary 需配置，备份作业由主库自动同步到备库）
- `dm_installer validate` 支持校验 `dw.toml`（节点/`oguid`/端口冲突/SSH 凭证/`instance_name` 唯一性/`backup_path` 等）

### 变更

- 主版本号升级至 2.0.0：单机安装与主备集群安装并列为两条主路径
- `dw.toml` 节点默认值与 `standalone.toml` 对齐，仅集群专属字段保留差异

## [1.2.3] - 2026-06-21

### 修复

- **v1.2.2 仍未生效**：target-scoped 的 `CARGO_TARGET_*_RUSTFLAGS` 被 `dist build` 内部设置的裸 `RUSTFLAGS` 整体覆盖。改为在构建脚本中按目标条件直接 `export RUSTFLAGS`，确保静态链接 flag 真正传给 rustc

## [1.2.2] - 2026-06-21

### 修复

- **v1.2.1 仍未完全修复 musl 动态链接问题**：单独开启 `crt-static` 与 musl target 默认的 PIE 结合产生 static-pie，旧版 musl-gcc 对其支持不完整，实际仍是动态链接。现已同时关闭 PIE 并在 CI 中通过 `RUSTFLAGS` 强制生效，产出完全静态的二进制

## [1.2.1] - 2026-06-21

### 修复

- **musl 发布产物实际为动态链接**：v1.2.0 的 `x86_64-unknown-linux-musl` / `aarch64-unknown-linux-musl` 二进制因未显式开启 `crt-static`，依赖 `/lib/ld-musl-*.so.1` 动态加载器，在 CentOS 等 glibc 发行版上报 `bad ELF interpreter`，现已修复为完全静态链接

## [1.2.0] - 2026-06-21

### 新增

- **单机安装支持 `backup_path` 配置**：`standalone.toml` 的 `[install]` 段新增可选 `backup_path` 字段，用于指定数据库备份目录
- **安装完成后的配置建议检查**：自动检查备份目录是否配置、备份/归档目录是否与数据目录路径重叠（同盘风险），并在安装完成后输出提醒

## [1.1.0] - 2026-06-16

### 新增

- **`install.sh` 支持非 root sudo 用户**：具有 sudo 权限的普通用户可直接运行安装脚本，特权操作自动通过 `sudo` 执行

### 变更

- **Linux 二进制改用 musl 静态链接**：无 glibc 版本依赖，可在任意 Linux 发行版运行
- **SSH 远程单机安装重构**：提升远端推送稳定性、错误提示和断点续传可靠性
- **移除归档模式**：`install.sh` 不再默认开启归档（面向开发环境）
- 主备集群配置类型从 `primary-standby` 重命名为 `dw`：`config.toml` 中 `type = "dw"`，对应特有配置文件改为 `dw.toml`
- `init` 子命令扁平化：`dm_installer init dw`（原 `dm_installer init cluster primary-standby`）

### 修复

- `install.sh`：多处 `sudo` 权限问题修复（`dminit` 检查、临时脚本写入、rollback 清理、进程残留）
- macOS 平台 `HOME` 路径展开及 SFTP 权限设置兼容（条件编译）

## [1.0.0] - 2026-06-14

### 新增

- **`install.sh` 一行安装**：`curl | bash` 单命令在 Linux 上完成 DM8 静默安装，无需编译、无需额外依赖
  - 自动检测平台架构（x86_64 / aarch64 / loongarch64 / mips64el / sw_64）和 CPU 型号（Hygon、飞腾、鲲鹏等）
  - 从 `versions.txt` 精确匹配下载链接，含 OS 回退逻辑
  - 自动生成满足达梦密码策略的随机 SYSDBA / SYSAUDITOR 密码并打印凭证卡片
  - 注册 `DmAPService` 和 `DmService<INSTANCE>.service` systemd 服务并自动启动
- **`dm_installer` 二进制**：TOML 配置驱动的精细安装工具
  - 单机静默安装：自动下载匹配当前平台的 DM8 安装包（支持 Kylin、UOS、CentOS、RHEL、Ubuntu、Debian 等发行版）
  - SSH 远程安装：`[ssh_target]` 配置后自动推送并在远端执行安装，含上传进度条和安装 spinner
  - 断点续传：安装中断后重跑自动从检查点恢复，已完成步骤不重复
  - 主备集群部署：批量 SSH 推送、`dm.ini` / `dmarch.ini` / `dmmal.ini` / `dmwatcher.ini` 自动生成与分发
  - `dm_installer init standalone / dw / rws / dsc` 生成带注释的配置模板
  - `dm_installer validate` 验证配置文件语法与语义
  - 配置语义校验：page_size / charset / extent_size / port 枚举值域检查
  - 安装引导（guide）：未找到 config.toml 时打印分步操作提示

### 修复

- Kylin V10 SP1（Lance）现在正确识别，不再误选 SP3 安装包
- SSH 连接失败自动重试，可配置重试次数与间隔
- `HOME` 路径展开兼容性修复
- 改用 `unzip DMInstall.bin` 提取 dmdbms，放弃不稳定的 `-q xml` 静默安装方式
- SHA-256 校验和验证

### 平台

- `install.sh`：Linux x86_64 / aarch64（Kylin V10、UOS 20、CentOS 7、RHEL 7+、Ubuntu 20+）
- `dm_installer` 预编译二进制：Linux x86_64/aarch64（glibc ≥ 2.23）、macOS Apple Silicon

[Unreleased]: https://github.com/guangl/dm-database-installer/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/guangl/dm-database-installer/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/guangl/dm-database-installer/releases/tag/v1.0.0
