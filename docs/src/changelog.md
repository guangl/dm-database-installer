# 更新日志

## [Unreleased]

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
- `init` 子命令扁平化：`dm-installer init dw`（原 `dm-installer init cluster primary-standby`）

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
- **`dm-installer` 二进制**：TOML 配置驱动的精细安装工具
  - 单机静默安装：自动下载匹配当前平台的 DM8 安装包（支持 Kylin、UOS、CentOS、RHEL、Ubuntu、Debian 等发行版）
  - SSH 远程安装：`[ssh_target]` 配置后自动推送并在远端执行安装，含上传进度条和安装 spinner
  - 断点续传：安装中断后重跑自动从检查点恢复，已完成步骤不重复
  - 主备集群部署：批量 SSH 推送、`dm.ini` / `dmarch.ini` / `dmmal.ini` / `dmwatcher.ini` 自动生成与分发
  - `dm-installer init standalone / dw / rws / dsc` 生成带注释的配置模板
  - `dm-installer validate` 验证配置文件语法与语义
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
- `dm-installer` 预编译二进制：Linux x86_64/aarch64（glibc ≥ 2.23）、macOS Apple Silicon

[Unreleased]: https://github.com/guangl/dm-database-installer/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/guangl/dm-database-installer/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/guangl/dm-database-installer/releases/tag/v1.0.0
