# dm-installer

达梦数据库（DM8）自动化安装工具——开发者一行命令搞定本地环境，DBA 用配置文件完成生产集群部署。

[![CI](https://github.com/guangl/dm-database-installer/actions/workflows/ci.yml/badge.svg)](https://github.com/guangl/dm-database-installer/actions/workflows/ci.yml)
[![Release](https://github.com/guangl/dm-database-installer/actions/workflows/release.yml/badge.svg)](https://github.com/guangl/dm-database-installer/actions/workflows/release.yml)
[![Crates.io](https://img.shields.io/crates/v/dm-database-installer.svg)](https://crates.io/crates/dm-database-installer)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

**[📖 文档站](https://guangl.github.io/dm-database-installer/)** | [快速开始](#快速开始) | [配置参考](#配置参考) | [集群部署](#集群部署)

---

## 特性

- **单机静默安装**：自动下载适配当前平台的 DM8 安装包，无需手动选版本
- **SSH 远程安装**：在控制机上配置一次，推送安装到目标服务器
- **主备集群部署**：一条命令完成主备节点批量部署与配置同步
- **断点续传**：安装中断后重跑自动从检查点恢复，不重复已完成步骤
- **配置驱动**：TOML 配置文件，所有参数有明确默认值，最少填两行即可运行
- **兼容性**：Linux 二进制采用 musl 静态链接，无 glibc 版本依赖，可在任意 Linux 发行版运行；同时提供 macOS Apple Silicon 原生二进制

## 安装

### 方式一：一行命令直接安装 DM 数据库（开发者推荐）

纯 shell 脚本，无需 Rust，无需任何外部依赖，`curl | bash` 即可在本地拉起 DM8 环境：

```sh
curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh | bash
```

所有安装参数均可通过同名环境变量覆盖，无需修改脚本：

```sh
# 自定义安装目录和端口
DM_INSTALL_PATH=/opt/dmdbms DM_PORT=5237 bash -c \
  "$(curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh)"
```

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `DM_INSTALL_PATH` | `/home/dmdba/dmdbms` | 程序安装目录 |
| `DM_DATA_PATH` | `$DM_INSTALL_PATH/data` | 数据文件目录 |
| `DM_PORT` | `5236` | 监听端口 |
| `DM_INSTANCE` | `DMSERVER` | 实例名称 |
| `DM_DB_NAME` | `DAMENG` | 数据库名称 |
| `DM_PAGE_SIZE` | `32` | 页大小（KB）：4 / 8 / 16 / 32 |
| `DM_EXTENT_SIZE` | `32` | 区段大小（页数）：16 / 32 |
| `DM_CHARSET` | `0` | 字符集：0=GB18030  1=UTF-8  2=EUC-KR |
| `DM_CASE_SENSITIVE` | `Y` | SQL 标识符大小写敏感：Y / N |

> 仅支持 Linux（x86_64 / aarch64）。需要 root 权限或具有 sudo 权限的普通用户。安装完成后会输出随机生成的 SYSDBA / SYSAUDITOR 密码，请妥善保存。

### 方式二：安装 dm-installer 管理工具（DBA / 生产环境推荐）

适合需要自定义参数、SSH 远程部署、主备集群等精细化场景。

**Linux / macOS**
```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/guangl/dm-database-installer/releases/latest/download/dm-database-installer-installer.sh | sh
```

### 方式三：从 crates.io 安装

```sh
cargo install dm-database-installer
```

### 方式四：从源码编译

```sh
git clone https://github.com/guangl/dm-database-installer.git
cd dm-database-installer
cargo build --release
# 二进制位于 target/release/dm-installer
```

## 快速开始

### 单机安装（本地）

```sh
# 1. 生成配置模板
dm-installer init standalone

# 2. 按需编辑（默认值通常够用）
# vim standalone.toml

# 3. 安装（自动下载安装包）
dm-installer install
```

### 单机安装（SSH 远程）

```sh
dm-installer init standalone
```

在生成的 `standalone.toml` 中取消注释 `[ssh_target]` 部分：

```toml
[ssh_target]
host = "192.168.1.100"
ssh_port = 22
user = "root"
# password 不填则运行时提示输入
```

```sh
dm-installer install
```

### 主备集群

```sh
dm-installer init dw
# 编辑 config.toml + dw.toml，填写节点 IP 和 SSH 凭证
dm-installer install
```

## 配置参考

所有安装都依赖两个配置文件，由 `dm-installer init` 自动生成模板。

### `config.toml`（通用配置）

```toml
# 安装类型：standalone / dw / rws / dsc
type = "standalone"

# 本地安装包路径（不填则自动下载匹配当前平台的版本）
# installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

# 日志级别：trace / debug / info / warn / error
log_level = "info"
```

### `standalone.toml`（单机特有配置）

```toml
[install]
install_path = "/home/dmdba/dmdbms"
data_path    = "/home/dmdba/dmdbms/data"
# backup_path = "/home/dmdba/dmdbms/backup"  # 数据库备份目录，强烈建议配置；未配置会在安装完成后提醒

[instance]
instance_name  = "DMSERVER"
port           = 5236
page_size      = 32   # 页大小 KB：4 / 8 / 16 / 32
charset        = 1    # 0=GB18030  1=UTF-8  2=EUC-KR
case_sensitive = true
extent_size    = 32   # 区段大小（页数）：16 / 32

# [ssh_target]        # 省略则本地安装
# host       = "192.168.1.100"
# ssh_port   = 22
# user       = "root"
```

## 集群部署

| 集群类型 | 命令 | 状态 |
|---------|------|------|
| 主备（DW）| `dm-installer init dw` | ✅ 支持 |
| 读写分离（RWS）| `dm-installer init rws` | ✅ 支持 |
| 共享存储（DSC）| `dm-installer init dsc` | ✅ 支持 |
| 数据保护集群（DPC）| — | 🚧 开发中 |

## 子命令

```
dm-installer install                    安装（读取 config.toml 自动判断类型）
dm-installer install --package PATH     指定本地安装包路径（跳过下载）
dm-installer install --url URL          指定自定义下载链接
dm-installer install --checksum SHA256  校验安装包 SHA-256
dm-installer validate [PATH]            验证配置文件语法与语义，不执行安装
dm-installer init standalone            生成单机配置模板
dm-installer init dw                    生成主备（DW）集群配置模板
dm-installer init rws                   生成读写分离（RWS）集群配置模板
dm-installer init dsc                   生成共享存储（DSC）集群配置模板
dm-installer status                     查询本地及远程节点运行状态
dm-installer --help                     查看帮助
```

## 支持平台

| 平台 | 架构 | 备注 |
|------|------|------|
| Linux | x86_64 | musl 静态链接，无 glibc 依赖 |
| Linux | aarch64 | musl 静态链接，无 glibc 依赖 |
| macOS | Apple Silicon | 11.0+ |

## 开发

```sh
cargo test          # 运行单元测试
cargo clippy        # Lint 检查
cargo run -- --help # 本地运行
```

## 项目状态

此仓库的最初需求（Phase 1 单机静默安装脚本）已基本完成。其余功能（SSH 远程安装、主备/读写分离/DSC 集群部署等）视情况更新，不排除不再继续开发的可能。

## License

[Apache-2.0](LICENSE)
