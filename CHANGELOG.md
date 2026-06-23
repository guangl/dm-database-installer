# Changelog

所有值得记录的版本变更都在本文件中。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [2.0.0] - 2026-06-23

### 新增

- **主备集群（DW）安装支持**：`dm_installer init dw` 生成 `config.toml` + `dw.toml` 配置模板，`dm_installer install` 按达梦官方[数据守护搭建文档](https://eco.dameng.com/document/dm/zh-cn/pm/data-guard-construction.html)的步骤顺序自动完成整套主备搭建：
  - 连接预检 → 环境准备（dmdba 用户/SELinux/内核参数等，与单机安装共用逻辑）→ 上传安装包 → 静默安装 → `dminit` 初始化
  - **备份还原同步备库数据**：在 primary 上用 `dmrman` 做一次脱机全量备份，经控制机中转打包传输到每个 standby，再执行 RESTORE + RECOVER 建立一致的数据基线
  - 分发 `dmmal.ini`/`dmarch.ini`/`dmwatcher.ini`，并在 `dm.ini` 中补充 `MAL_INI`/`ARCH_INI`/`DW_INACTIVE_INTERVAL`/`ENABLE_OFFLINE_TS`/`RLOG_SEND_APPLY_MON` 等守护参数
  - 以 **Mount 模式**启动 `dmserver`（`dmserver dm.ini mount`），通过 disql 执行 `sp_set_oguid` 与 `ALTER DATABASE PRIMARY/STANDBY`（primary 先于 standby）
  - **三进程均注册为 systemd 服务**：`dmserver`（`DmService<实例名>`）、`dmwatcher`（`DmWatcherService<实例名>`）、`dmmonitor`（`DmMonitorService<实例名>`）均通过 `dm_service_installer.sh` 注册，随系统自启；`dmserver` 注册时传 `-m mount`，使每次启动均以 Mount 模式进入，由 dmwatcher 负责切换为 Open
  - **监视器部署在 standby 节点**：`dmmonitor` 默认运行于第一个 standby 节点，避免与 primary 共置（primary 故障时监视器仍可仲裁）；集群无 standby 时 fallback 到 primary
  - 启动 `dmwatcher` 守护进程（自动将 Mount 状态实例切换为 Open）与 `dmmonitor` 确认监视器
  - 配置备份作业、开启 SQL 日志（SVR_LOG）、应用官方自动参数调整脚本（调整后以 Mount 模式重启 dmserver 生效）
- **集群断点续传**：按节点维度的检查点续传覆盖以上每一个独立步骤，中断后重跑 `dm_installer install` 会跳过已完成的节点和步骤（启动顺序步骤因 primary 优先的强约束仍整体重跑）
- `dw.toml` 节点新增 `[nodes.backup]` 备份作业配置段（字段与单机 `standalone.toml` 的 `[backup]` 一致）
- `dw.toml` 的 `oguid` 字段可省略，默认值为执行 `dm_installer init dw` 当天的 `YYYYMMDD` 数字（如 `20260623`），满足达梦对 oguid 全局唯一的要求；有效范围 0–2147483647
- `dm_installer validate` 支持校验 `dw.toml`：节点列表非空、恰好一个 primary、`oguid` 范围、`mal_port` 不与 `port` 冲突、SSH 凭证完整性、`instance_name` 集群内唯一、各节点 `backup_path` 已配置
- `CommandRunner` 新增 `sftp_read`（SSH/本地/Mock 三种实现均已支持），用于控制机中转两个远端节点间的文件传输
- 主备集群（dw）安装包来源与单机一致支持三选一：本地路径、下载链接、或都不填自动检测平台下载（按 primary 节点平台检测，下载后的同一份包推送到所有节点；下载结果按 oguid 缓存进集群 checkpoint，断点续传时跳过重复下载）

### 变更

- 主版本号升级至 2.0.0：单机安装（standalone）与主备集群安装（dw）并列为本工具的两条主路径，对齐 PROJECT.md 中“开发者单机环境 / DBA 生产集群”的双用户定位
- `dw.toml` 节点默认值（`install_path`/`data_path`/`page_size`/`charset`/`extent_size`）改为与 `standalone.toml` 一致，仅集群专属字段（端口、角色、SSH）保留差异
- `install::steps::param_tune` 拆分出不含重启逻辑的 `apply()`，供集群安装在 Mount 模式重启场景下复用

## [1.2.3] - 2026-06-21

### 修复

- **v1.2.2 仍未生效**：验证发现 `CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS` 这类 target-scoped 环境变量被 `dist build` 内部设置的裸 `RUSTFLAGS` 整体覆盖（cargo 的优先级是"整体覆盖"而非"合并"），导致静态链接 flag 从未真正传给 rustc。现改为在构建脚本里按目标条件判断后直接 `export RUSTFLAGS`（裸变量，最高优先级），确保 musl 目标一定拿到 `+crt-static` 与 `relocation-model=static`

## [1.2.2] - 2026-06-21

### 修复

- **v1.2.1 仍未完全修复 musl 动态链接问题**：单独开启 `crt-static` 在较新 Rust 版本上与 musl target 默认的 PIE 结合产生 static-pie，Ubuntu 22.04 自带的 musl-gcc 对 static-pie 支持不完整，实际仍产出动态链接二进制。现在同时显式关闭 PIE（`-C relocation-model=static`），并在 CI 的 `RUSTFLAGS` 环境变量中直接设置，确保构建一定生效，产出传统的完全静态二进制

## [1.2.1] - 2026-06-21

### 修复

- **musl 发布产物实际为动态链接**：`x86_64-unknown-linux-musl` / `aarch64-unknown-linux-musl` 默认未开启 `crt-static`，导致 v1.2.0 发布的二进制依赖 `/lib/ld-musl-*.so.1` 动态加载器，在 CentOS 等 glibc 发行版上报 `bad ELF interpreter`。`.cargo/config.toml` 显式追加 `-C target-feature=+crt-static`，确保产出完全静态链接的二进制

## [1.2.0] - 2026-06-21

### 新增

- **单机安装支持 `backup_path` 配置**：`standalone.toml` 的 `[install]` 段新增可选 `backup_path` 字段，用于指定数据库备份目录
- **安装完成后的配置建议检查**：自动检查备份目录是否配置、备份/归档目录是否与数据目录路径重叠（同盘风险），并在安装完成后输出提醒；检查逻辑独立为 `install::advisory` 模块，便于后续集群模式扩展专属规则

## [1.1.0] - 2026-06-16

### 新增

- **`install.sh` 支持非 root sudo 用户**：具有 sudo 权限的普通用户现在可直接运行安装脚本，特权操作自动通过 `sudo` 执行，无需切换到 root

### 变更

- **Linux 二进制改用 musl 静态链接**：`dm-installer` Linux 预编译包完全静态链接，无 glibc 版本依赖，可在任意 Linux 发行版运行
- **SSH 远程单机安装重构**：`standalone` 模块全面重写，提升远端推送稳定性、错误提示和断点续传可靠性
- **移除归档模式**：`install.sh` 不再开启归档（面向开发环境，去掉不必要的复杂度）

### 修复

- `install.sh`：`VERSIONS_URL` 改回 GitHub 地址，Gitee mirror 通过 CI 的 `sed` 替换保持同步
- `install.sh`：非 root 用户执行 `dminit` 可执行性检查时误报文件不存在（改用 `sudo test -x`）
- `install.sh`：非 root 用户写 dmdba 临时脚本时权限不足（改用 `sudo tee`）
- `install.sh`：非 root 用户 rollback 删除安装/数据目录权限不足（`rm -rf` 补加 `sudo`）
- `install.sh`：rollback 时仅在服务已注册的情况下才 kill dmap/dmserver，导致安装前期失败时进程残留（现在始终执行 kill）
- `sftp_set_permissions` 加 `#[cfg(unix)]` 条件编译，修复非 Unix 环境下的 clippy 错误

### 平台

- `dm-installer` Linux 预编译二进制：musl 静态链接，无 glibc 依赖（x86_64 / aarch64）
- `dm-installer` macOS：Apple Silicon（保持不变）

## [1.0.0] - 2026-06-14

### 新增

- **`install.sh` 一行安装（Phase 1）**：`curl | bash` 单命令在 Linux 上完成达梦数据库静默安装，无需编译、无需额外依赖
  - 自动检测平台架构（x86_64 / aarch64 / loongarch64 / mips64el / sw_64）和 CPU 型号（Hygon、飞腾、鲲鹏等）
  - 从 `versions.txt` 精确匹配下载链接，含 OS 回退逻辑
  - 自动生成满足达梦密码策略的随机 SYSDBA / SYSAUDITOR 密码并输出凭证卡片
  - 注册 `DmAPService` 和 `DmService<INSTANCE>.service` systemd 服务并自动启动
  - 安装完成后打印连接命令与服务状态查看命令
- **`dm-installer` 二进制（Phase 2）**：TOML 配置驱动的精细安装工具，面向 DBA / 运维
  - 单机静默安装：自动下载适配当前平台的 DM8 安装包（x86_64 / aarch64，支持 Kylin、UOS、CentOS、RHEL、Debian 等发行版）
  - SSH 远程安装：`[ssh_target]` 配置后自动推送并在远端执行安装，含上传进度条和安装 spinner
  - 断点续传（checkpoint）：安装中断后重跑自动从最近检查点恢复
  - 安装完成后自动生成随机 SYSDBA / SYSAUDITOR 密码并输出
  - 主备集群（Primary-Standby）部署：批量 SSH 推送、dm.ini / dmarch.ini / dmmal.ini / dmwatcher.ini 配置生成与同步
  - `dm-installer init standalone` / `cluster primary-standby` 生成配置模板
  - `dm-installer validate` 验证配置文件语法与语义
  - 配置文件语义校验：page_size / charset / extent_size / port 枚举值域检查
  - 安装引导（guide）：未找到 config.toml 时打印分步操作提示

### 修复

- Kylin V10 SP1（Lance）现在正确识别，不再误选 SP3 安装包
- SSH 连接失败自动重试，可配置重试次数与间隔
- macOS 兼容 `HOME` 路径展开问题
- 改用 `unzip DMInstall.bin` 提取 dmdbms，放弃不稳定的 `-q xml` 静默安装方式
- SHA-256 校验和验证
- 降低 glibc 最低要求至 2.23（改用 cargo-zigbuild）

### 平台

- **`install.sh`**：Linux x86_64 / aarch64（含 Kylin V10、UOS 20、CentOS 7、RHEL 7+、Ubuntu 20+）
- **`dm-installer` 预编译二进制**：Linux x86_64/aarch64（glibc ≥ 2.23）、macOS Apple Silicon
- CI/CD：GitHub Actions + cargo-dist + cargo-zigbuild 精确控制 glibc 版本

[Unreleased]: https://github.com/guangl/dm-database-installer/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/guangl/dm-database-installer/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/guangl/dm-database-installer/compare/v0.1.0...v1.0.0
[0.1.0]: https://github.com/guangl/dm-database-installer/releases/tag/v0.1.0
