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

# DM8 安装包本地路径或下载链接，与 installer_url 二选一
# 都不填则自动检测平台并下载（集群模式按 primary 节点平台检测）
# 确定后会将此文件推送到各节点再静默安装
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
| `oguid` | 当天 `YYYYMMDD`（如 `20260623`） | 守护系统全局唯一标识，同一守护系统内所有实例必须一致，范围 0–2147483647 |
| `mon_confirm` | `true` | `true`=确认监视器（参与自动切换仲裁）；`false`=通知监视器（仅通知，不参与仲裁） |

### [[nodes]] — 节点配置

每个节点必须包含以下字段：

| 键 | 默认值 | 说明 |
|----|--------|------|
| `role` | — | `"primary"` 或 `"standby"`（必填，且只能有一个 primary） |
| `host` | — | 节点 IP 或域名（必填） |
| `instance_name` | — | 实例名称，同一集群内必须唯一（必填） |
| `install_path` | `/home/dmdba/dmdbms` | DM 程序安装目录（与 standalone 默认一致） |
| `data_path` | `/home/dmdba/dmdbms/data` | 数据文件目录（与 standalone 默认一致） |
| `arch_path` | `{data_path}/arch` | 本节点本地归档目录 |
| `port` | `5236` | 数据库监听端口 |
| `mal_port` | `5237` | MAL 通信端口（不能与 port 相同） |
| `dw_port` | `5238` | 数据守护监听端口（即 MAL_DW_PORT，dmmonitor 也用此端口） |
| `inst_dw_port` | `5239` | 实例向守护进程注册的端口 |
| `page_size` | `32` | 页大小（KB）：`4` / `8` / `16` / `32` |
| `charset` | `1` | 0=GB18030  1=UTF-8  2=EUC-KR |
| `case_sensitive` | `true` | SQL 标识符大小写敏感 |
| `extent_size` | `32` | 区段大小（页数）：`16` / `32` |
| `sync_mode` | `"realtime"` | 仅 standby 节点有意义。`"realtime"`（实时备库，REALTIME，加入 dmwatcher 全局守护组、参与监视器仲裁与自动切换）/ `"sync"`（同步备库，SYNC，本地守护）/ `"async"`（异步备库，ASYNC，本地守护，需配套 `arch_timer_name`） |
| `arch_timer_name` | `"RT_TIMER"` | 仅 `sync_mode = "async"` 时生效，引用 DM 内置定时器名 |

### [nodes.backup] — 节点备份作业配置

字段与 [standalone.toml 的 `[backup]` 段](#standalonetoml) 完全一致，每个节点独立配置（必填，用于在该节点上创建全备/增量备份作业）。

| 键 | 默认值 | 说明 |
|----|--------|------|
| `backup_path` | — | 数据库备份目录（必填） |
| `retain_days` | `15` | 备份保留天数，至少 15 天 |
| `full_backup_interval_days` | `7` | 全量备份间隔天数 |
| `full_backup_time` | `"02:00:00"` | 全量备份执行时间 |
| `incr_backup_time` | `"02:00:00"` | 增量备份执行时间 |
| `clean_time` | `"05:00:00"` | 过期备份清理执行时间 |

### [nodes.ssh] — 节点 SSH 凭证

每个节点的 `[nodes.ssh]` 节描述控制机如何 SSH 到该节点。

| 键 | 默认值 | 说明 |
|----|--------|------|
| `user` | — | SSH 用户名（必填） |
| `identity_file` | — | SSH 私钥路径（与 password 二选一） |
| `password` | — | SSH 密码（与 identity_file 二选一；不填则安装时提示） |

### [mal] — MAL 通信层（dmmal.ini，可选）

每节点的 `MAL_INST_NAME`/`MAL_HOST`/`MAL_PORT` 等由 `[[nodes]]` 自动推导，以下为全局参数，省略则使用默认值。

| 键 | 默认值 | 说明 |
|----|--------|------|
| `mal_check_interval` | `60` | MAL 链路检测间隔（秒），0=禁用，范围 0–1800 |
| `mal_conn_fail_interval` | `60` | 判定连接失败的时长阈值（秒），范围 2–1800 |
| `mal_login_timeout` | `60` | 节点间登录超时（秒），范围 3–1800 |
| `mal_buf_size` | `100` | 单连接缓冲区上限（MB），0=不限 |
| `mal_sys_buf_size` | `0` | 系统全局 MAL 内存上限（MB），0=不限 |
| `mal_compress_level` | `0` | 压缩级别：0=不压缩，1–9=lz，10=snappy；所有节点必须一致 |

### [watcher] — 数据守护（dmwatcher.ini，可选）

| 键 | 默认值 | 说明 |
|----|--------|------|
| `dw_mode` | `"MANUAL"` | `"MANUAL"`（需人工介入）/ `"AUTO"`（故障自动切换） |
| `dw_error_time` | `60` | 守护进程故障确认时间（秒），范围 3–1800 |
| `inst_error_time` | `60` | 数据库实例故障确认时间（秒），范围 3–1800 |
| `inst_recover_time` | `60` | 备库恢复检测间隔（秒），范围 3–86400 |
| `inst_auto_restart` | `0` | 实例崩溃后自动重启：0=否，1=是 |
| `inst_restart_cnt` | `0` | 最大连续重启次数，0=不限制 |
| `dw_open_force_timeout` | `0` | MANUAL 模式无监视器时强制 Open 超时（秒），0=永久等待 |
| `dw_failover_force` | `1` | 无监视器时允许主库强制 Open：1=允许，0=禁止 |
| `dw_reconnect` | `1` | 断链重连策略：0=不重连，1=重连继续守护，2=重连降为 OPEN |
| `rlog_send_threshold` | `0` | 实时归档发送延迟告警阈值（秒），0=不告警 |
| `rlog_apply_threshold` | `0` | 备库日志应用延迟告警阈值（秒），0=不告警 |
| `inst_service_ip_check` | `0` | 检测主库对外服务 IP 可达性：0=不检测，1=检测 |

### [arch] — 日志归档（dmarch.ini，可选）

| 键 | 默认值 | 说明 |
|----|--------|------|
| `arch_wait_apply` | `1` | 同步备库是否等待日志应用后再回应主库：1=等待，0=不等待 |
| `arch_reserve_time` | `0` | 本地归档保留时长（分钟），0=不自动清理 |
| `arch_send_policy` | `0` | 主库发送策略：0=立即等待备库响应，1=先写本地再发送 |
| `arch_recover_time` | `60` | 同步备库归档状态检测间隔（秒），范围 1–86400 |
| `arch_file_size` | `1024` | 本地归档单文件大小（MB），范围 64–2048 |
| `arch_space_limit` | 不填=自动 | 本地归档总空间上限（MB）：不填时自动取归档目录所在磁盘总容量的 20%，探测失败时退回默认值 20480（20GB）；显式填 0=不限；其他正整数=固定上限 |

### [monitor] — 确认监视器（dmmonitor.ini，可选）

| 键 | 默认值 | 说明 |
|----|--------|------|
| `mon_log_path` | `"."` | 监视器日志输出目录 |
| `mon_log_interval` | `60` | 日志刷新间隔（秒），范围 1–600 |
| `mon_log_file_size` | `32` | 单个日志文件大小上限（MB），范围 1–2048 |
| `mon_log_space_limit` | `4096` | 日志总空间上限（MB），0=不限 |

**完整示例：**

```toml
# 守护系统全局唯一标识（主备节点必须相同），省略则默认为当天 YYYYMMDD
oguid = 453331

# ── 主节点 ────────────────────────────────────────────────
[[nodes]]
role          = "primary"
host          = "192.168.1.10"
instance_name = "DM01"
install_path  = "/home/dmdba/dmdbms"
data_path     = "/home/dmdba/dmdbms/data"
port          = 5236
mal_port      = 5237
dw_port       = 5238
inst_dw_port  = 5239
page_size     = 32
charset       = 1
case_sensitive = true
extent_size   = 32

[nodes.backup]
backup_path = "/home/dmdba/dmdbms/backup"
retain_days = 15
full_backup_interval_days = 7
full_backup_time = "02:00:00"
incr_backup_time = "02:00:00"
clean_time        = "05:00:00"

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
# password   = "your_password"   # 与 identity_file 二选一

# ── 备节点 ────────────────────────────────────────────────
[[nodes]]
role          = "standby"
host          = "192.168.1.11"
instance_name = "DM02"
# sync_mode 可省略，默认 "realtime"；也可设为 "sync" 或 "async"（async 需配套 arch_timer_name）
install_path  = "/home/dmdba/dmdbms"
data_path     = "/home/dmdba/dmdbms/data"
port          = 5236
mal_port      = 5237
dw_port       = 5238
inst_dw_port  = 5239
page_size     = 32
charset       = 1
case_sensitive = true
extent_size   = 32

# standby 节点无需配置 [nodes.backup]，备份作业由主库同步过来

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"

# ── [mal]/[watcher]/[arch]/[monitor] 均可省略，全部走默认值 ──
# [mal]
# mal_check_interval = 60
#
# [watcher]
# dw_mode = "MANUAL"
#
# [arch]
# arch_space_limit = 20480   # 不填则自动取磁盘总容量的 20%
#
# [monitor]
# mon_log_space_limit = 4096
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
