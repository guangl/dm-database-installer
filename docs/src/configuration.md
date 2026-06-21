# 配置参考

`dm_installer` 使用两个配置文件，均放在同一目录下，由 `dm_installer init` 生成带注释的模板。

| 文件 | 说明 |
|------|------|
| `config.toml` | 通用配置：安装类型、安装包路径、日志级别 |
| `standalone.toml` / `dw.toml` / `rws.toml` / `dsc.toml` | 各类型特有配置 |

执行 `dm_installer install` 时，工具从当前目录读取 `config.toml`，再根据其中的 `type` 字段加载对应的特有配置文件。

---

## config.toml

```toml
# 安装类型（必填）
# standalone  — 单机安装
# dw          — 主备集群（Data Watch）
# rws         — 读写分离集群（开发中）
# dsc         — 共享存储集群（开发中）
type = "standalone"

# DM8 安装包本地路径
# 单机：留空则自动下载匹配当前平台的版本
# 集群：必填，工具会将此文件推送到各节点
# installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

# 日志级别：trace / debug / info / warn / error
log_level = "info"
```

---

## standalone.toml

### [install] — 安装路径

| 键 | 默认值 | 说明 |
|----|--------|------|
| `install_path` | `/home/dmdba/dmdbms` | DM 程序安装目录 |
| `data_path` | `/home/dmdba/dmdbms/data` | 数据文件目录 |
| `backup_path` | — | 数据库备份目录（可选）。未配置、或与 `data_path`/归档目录路径重叠时，安装完成后会输出配置建议提醒 |

### [instance] — 实例参数

| 键 | 默认值 | 有效值 | 说明 |
|----|--------|--------|------|
| `instance_name` | `DMSERVER` | 任意字符串 | 实例名称 |
| `port` | `5236` | 1–65535 | 数据库监听端口 |
| `page_size` | `32` | `4` / `8` / `16` / `32` | 页大小（KB），建议生产环境使用 32 |
| `charset` | `1` | `0` / `1` / `2` | 0=GB18030  1=UTF-8  2=EUC-KR |
| `case_sensitive` | `true` | `true` / `false` | SQL 标识符大小写敏感 |
| `extent_size` | `32` | `16` / `32` | 区段大小（页数） |

### [ssh_target] — SSH 远程目标（可选）

省略此节时在本机安装。填写后，工具会通过 SSH 将安装包推送到目标服务器并在远端执行安装。

| 键 | 默认值 | 说明 |
|----|--------|------|
| `host` | — | 目标主机 IP 或域名（必填） |
| `ssh_port` | `22` | SSH 端口 |
| `user` | — | SSH 用户名（必填） |
| `password` | — | SSH 密码；不填则安装时提示输入（推荐） |
| `max_retries` | `3` | SSH 连接失败重试次数 |
| `retry_interval_secs` | `5` | 重试间隔（秒） |

**完整示例：**

```toml
[install]
install_path = "/home/dmdba/dmdbms"
data_path    = "/home/dmdba/dmdbms/data"
backup_path  = "/home/dmdba/dmdbms/backup"

[instance]
instance_name  = "DMSERVER"
port           = 5236
page_size      = 32
charset        = 1
case_sensitive = true
extent_size    = 32

[ssh_target]
host                = "192.168.1.100"
ssh_port            = 22
user                = "root"
max_retries         = 3
retry_interval_secs = 5
```

---

## dw.toml

主备集群特有配置。每个节点对应一条 `[[nodes]]` 条目。

### 顶层字段

| 键 | 默认值 | 说明 |
|----|--------|------|
| `oguid` | `453331` | 守护系统全局唯一标识，同一守护系统内所有实例必须一致，范围 0–2147483647 |

### [[nodes]] — 节点配置

每个节点必须包含以下字段：

| 键 | 默认值 | 说明 |
|----|--------|------|
| `role` | — | `"primary"` 或 `"standby"`（必填，且只能有一个 primary） |
| `host` | — | 节点 IP 或域名（必填） |
| `instance_name` | — | 实例名称，同一集群内必须唯一（必填） |
| `install_path` | `/opt/dmdbms` | DM 程序安装目录 |
| `data_path` | `/opt/dmdbms/data` | 数据文件目录 |
| `port` | `5236` | 数据库监听端口 |
| `mal_port` | `5237` | MAL 通信端口（不能与 port 相同） |
| `dw_port` | `5238` | 数据守护监听端口 |
| `inst_dw_port` | `5239` | 实例向守护进程注册的端口 |
| `page_size` | `8` | 页大小（KB）：`4` / `8` / `16` / `32` |
| `charset` | `0` | 0=GB18030  1=UTF-8  2=EUC-KR |
| `case_sensitive` | `true` | SQL 标识符大小写敏感 |
| `extent_size` | `16` | 区段大小（页数）：`16` / `32` |

### [nodes.ssh] — 节点 SSH 凭证

每个节点的 `[nodes.ssh]` 节描述控制机如何 SSH 到该节点。

| 键 | 默认值 | 说明 |
|----|--------|------|
| `user` | — | SSH 用户名（必填） |
| `identity_file` | — | SSH 私钥路径（与 password 二选一） |
| `password` | — | SSH 密码（与 identity_file 二选一；不填则安装时提示） |

**完整示例：**

```toml
# 守护系统全局唯一标识（主备节点必须相同）
oguid = 453331

# ── 主节点 ────────────────────────────────────────────────
[[nodes]]
role          = "primary"
host          = "192.168.1.10"
instance_name = "DMSVR01"
install_path  = "/opt/dmdbms"
data_path     = "/opt/dmdbms/data"
port          = 5236
mal_port      = 5237
dw_port       = 5238
inst_dw_port  = 5239
page_size     = 8
charset       = 0
case_sensitive = true
extent_size   = 16

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
# password   = "your_password"   # 与 identity_file 二选一

# ── 备节点 ────────────────────────────────────────────────
[[nodes]]
role          = "standby"
host          = "192.168.1.11"
instance_name = "DMSVR02"
install_path  = "/opt/dmdbms"
data_path     = "/opt/dmdbms/data"
port          = 5236
mal_port      = 5237
dw_port       = 5238
inst_dw_port  = 5239
page_size     = 8
charset       = 0
case_sensitive = true
extent_size   = 16

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
```

---

## 环境变量

### install.sh 快速安装脚本

`curl | bash` 脚本的所有参数均可通过同名环境变量覆盖：

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `DM_INSTALL_PATH` | `/home/dmdba/dmdbms` | 程序安装目录 |
| `DM_DATA_PATH` | `$DM_INSTALL_PATH/data` | 数据文件目录（默认跟随安装目录） |
| `DM_PORT` | `5236` | 监听端口 |
| `DM_INSTANCE` | `DMSERVER` | 实例名称 |
| `DM_DB_NAME` | `DAMENG` | 数据库名称 |
| `DM_PAGE_SIZE` | `32` | 页大小（KB）：4 / 8 / 16 / 32 |
| `DM_EXTENT_SIZE` | `32` | 区段大小（页数）：16 / 32 |
| `DM_CHARSET` | `0` | 字符集：0=GB18030  1=UTF-8  2=EUC-KR |
| `DM_CASE_SENSITIVE` | `Y` | SQL 标识符大小写敏感：Y / N |

用法示例：

```sh
# 只改安装目录（数据目录自动变为 /opt/dmdbms/data）
DM_INSTALL_PATH=/opt/dmdbms bash -c \
  "$(curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh)"

# 同时指定多个参数
DM_INSTALL_PATH=/opt/dmdbms DM_DATA_PATH=/data/dm DM_PORT=5237 bash -c \
  "$(curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh)"

# export 后 pipe 方式
export DM_INSTALL_PATH=/opt/dmdbms
curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh | bash
```

### dm_installer 工具

| 变量 | 说明 |
|------|------|
| `RUST_LOG` | 覆盖日志级别，例如 `RUST_LOG=debug dm_installer install` |
