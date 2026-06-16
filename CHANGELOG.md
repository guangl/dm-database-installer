# Changelog

所有值得记录的版本变更都在本文件中。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

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
