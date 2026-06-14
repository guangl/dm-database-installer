# dm-installer

达梦数据库（DM8）自动化安装工具。

**两类用户，一个工具：**

- **开发者**：一行命令在本地或测试服务器上拉起达梦实例，无需了解安装细节
- **DBA / 运维**：用 TOML 配置文件驱动生产级单机或主备集群的完整部署

## 核心特性

| 特性 | 说明 |
|------|------|
| 自动下载 | 根据当前 Linux 发行版和架构自动选择匹配的 DM8 安装包 |
| SSH 远程安装 | 在控制机上一键部署到目标服务器，含上传进度条 |
| 主备集群 | 批量推送安装包、生成并同步 dm.ini / dmarch.ini / dmmal.ini |
| 断点续传 | 安装中断后重跑从检查点恢复，不重复已完成步骤 |
| 配置驱动 | TOML 文件，最少两行即可运行；`dm-installer init` 生成模板 |
| 跨平台 | Linux x86_64 / aarch64、macOS x86_64 / Apple Silicon、Windows x86_64 |

## 源码

[github.com/guangl/dm-database-installer](https://github.com/guangl/dm-database-installer)
