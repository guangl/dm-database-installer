# Changelog

所有值得记录的版本变更都在本文件中。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [0.1.0] - 2026-06-14

### 新增

- 单机静默安装：自动下载适配当前平台的 DM8 安装包（x86_64 / aarch64，支持 Kylin、UOS、CentOS、RHEL、Debian 等发行版）
- SSH 远程安装：`[ssh_target]` 配置后自动推送并在远端执行安装，含上传进度条和安装 spinner
- 断点续传（checkpoint）：安装中断后重跑自动从最近检查点恢复
- 安装完成后自动生成随机 SYSDBA / SYSAUDITOR 密码并输出
- 主备集群（Primary-Standby）部署：批量 SSH 推送、dm.ini / dmarch.ini / dmmal.ini / dmwatcher.ini 配置生成与同步
- `dm-installer init standalone` / `cluster primary-standby` 生成配置模板
- `dm-installer validate` 验证配置文件语法与语义
- `dm-installer completions <shell>` 生成 bash / zsh / fish 补全脚本
- 配置文件语义校验：page_size / charset / extent_size / port 枚举值域检查
- 安装引导（guide）：未找到 config.toml 时打印分步操作提示

### 修复

- Kylin V10 SP1（Lance）现在正确识别，不再误选 SP3 安装包
- SSH 连接失败自动重试，可配置重试次数与间隔
- Windows 兼容 `HOME` 路径展开问题
- 改用 `unzip DMInstall.bin` 提取 dmdbms，放弃不稳定的 `-q xml` 静默安装方式
- SHA-256 校验和验证

### 平台

- 预编译二进制：Linux x86_64/aarch64（glibc ≥ 2.23）、macOS x86_64/Apple Silicon、Windows x86_64
- CI/CD：GitHub Actions + cargo-dist + cargo-zigbuild 精确控制 glibc 版本

[Unreleased]: https://github.com/guangl/dm-database-installer/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/guangl/dm-database-installer/releases/tag/v0.1.0
