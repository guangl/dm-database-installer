# dm-installer

达梦数据库（DM8）自动化安装工具，让开发者和运维人员都能用最少的操作完成 DM8 部署。

## 两类用户，两种入口

### 开发者 — 一行命令

本地或测试机快速拉起 DM8，不安装任何工具：

```sh
curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh | bash
```

纯 shell 脚本，自动检测平台、下载安装包、静默安装、注册 systemd 服务、输出密码凭证卡片。

### DBA / 运维 — TOML 配置驱动

安装 `dm-installer` 二进制，用配置文件描述目标环境，支持单机、SSH 远程、主备集群：

```sh
dm-installer init standalone   # 生成配置模板
dm-installer install           # 按模板安装
```

## 核心特性

| 特性 | 说明 |
|------|------|
| 自动选包 | 根据 Linux 发行版（Kylin / UOS / CentOS / RHEL / Ubuntu / Debian）和架构（x86_64 / aarch64）精确匹配 DM8 安装包 |
| SSH 远程安装 | 在控制机上一键部署到目标服务器，SFTP 上传带进度条 |
| 主备集群（DW） | 批量推送安装包、自动生成并同步 `dm.ini` / `dmarch.ini` / `dmmal.ini` / `dmwatcher.ini` |
| 断点续传 | 安装中断后重跑，自动从检查点恢复，已完成步骤不重复 |
| 配置驱动 | TOML 文件，`dm-installer init` 生成带注释的完整模板 |
| 配置校验 | `dm-installer validate` 在安装前检查所有语义约束 |
| 跨平台二进制 | Linux x86_64 / aarch64（musl 静态链接）、macOS Apple Silicon |

## 源码

[github.com/guangl/dm-database-installer](https://github.com/guangl/dm-database-installer)
