# 集群部署

## 支持的集群类型

| 类型 | init 命令 | config.toml type | 状态 |
|------|-----------|-----------------|------|
| 主备（DW） | `dm_installer init dw` | `dw` | ✅ 已支持 |
| 读写分离（RWS） | `dm_installer init rws` | `rws` | 🚧 开发中 |
| 共享存储（DSC） | `dm_installer init dsc` | `dsc` | 🚧 开发中 |

---

## 主备集群（DW）

### 前提条件

- 控制机可以通过 SSH 访问所有节点（22 端口，或自定义端口）
- 各节点已安装 `unzip`
- 提前在控制机上下载好 DM8 安装包（`.iso` 文件）

### 操作步骤

**第一步：生成配置模板**

```sh
dm_installer init dw
```

生成两个文件：

- `config.toml` — 通用配置，设置 `type = "dw"` 和安装包路径
- `dw.toml` — 节点列表，含各节点 IP、端口、SSH 凭证

**第二步：编辑 `config.toml`**

安装包来源三选一（与单机安装一致）：本地文件、自定义下载链接，或都不填自动检测 primary 节点平台后下载。下载/确定后会推送到各节点：

```toml
type = "dw"

# 三选一，都不填则自动检测下载
installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"
# installer_url = "https://download.example.com/dm8.zip"
```

**第三步：编辑 `dw.toml`**

填写节点信息（参考[配置参考 — dw.toml](configuration.md#dw-toml)中的字段说明）：

```toml
oguid = 453331

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

[[nodes]]
role          = "standby"
host          = "192.168.1.11"
instance_name = "DM02"
# sync_mode 可省略，默认 "realtime"（实时备库，参与自动切换）；也可设为 "sync"/"async"
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
```

**第四步：验证配置**

```sh
dm_installer validate
```

验证内容包括：

- 节点列表非空
- 恰好一个 `primary` 节点
- `oguid` 在 0–2147483647 范围内
- 每个节点 `mal_port` 不与 `port` 冲突
- 每个节点至少提供 `identity_file` 或 `password` 之一
- `page_size` 为 4/8/16/32，`charset` 为 0/1/2，`extent_size` 为 16/32
- `instance_name` 在集群内唯一
- primary 节点 `backup_path` 已配置且 `retain_days` 至少 15 天
- 异步备库（`sync_mode = "async"`）的 `arch_timer_name` 不能为空

`validate` 会彩色分栏打印出最终生效的完整配置（含 `dmwatcher.ini`/`dmmal.ini`/`dmarch.ini`/`dmmonitor.ini` 各参数及对应原始 ini 配置项），不实际执行安装，便于在部署前确认参数是否符合预期。

**第五步：部署**

```sh
dm_installer install
```

### 部署流程

工具按官方[数据守护搭建文档](https://eco.dameng.com/document/dm/zh-cn/pm/data-guard-construction.html)的步骤顺序自动完成：

1. **连接并预检**：并行检查所有节点的磁盘空间、端口占用、内存/CPU、SELinux/ulimit
2. **环境准备**：创建 dmdba 用户、关闭 SELinux/THP/防火墙、内核参数调优等（与单机安装相同）
3. **上传安装包**：逐节点 SFTP 上传（带进度条）
4. **静默安装**：在各节点执行 `DMInstall.bin` 静默安装 DM8
5. **dminit 初始化**：在各节点（primary 与 standby 均）执行 `dminit` 各自初始化一个新实例
6. **备份还原同步备库数据**：在 primary 上用 `dmrman` 做一次脱机全量备份，经控制机中转打包传输到每个 standby，再在 standby 上 `dmrman` 执行 RESTORE + RECOVER，使备库与主库数据基线一致
7. **分发主备守护配置**：
   - `dm.ini` 追加 `MAL_INI=1`、`ARCH_INI=1`、`DW_INACTIVE_INTERVAL`、`ENABLE_OFFLINE_TS`、`RLOG_SEND_APPLY_MON` 等参数
   - `dmmal.ini`（MAL 通信列表，各节点相同）
   - `dmarch.ini`（本地归档 + 指向对端的归档，类型按对端 `sync_mode` 决定：`realtime`→`REALTIME`，`sync`→`SYNC`，`async`→`ASYNC`+`ARCH_TIMER_NAME`；本地归档空间上限不填时自动取磁盘总容量的 20%）
   - `dmwatcher.ini`（数据守护配置；`DW_TYPE` 按本节点 `sync_mode` 决定：primary 与 realtime 备库为 `GLOBAL`，sync/async 备库为 `LOCAL`，不参与自动切换）
8. **以 Mount 方式启动数据库并设置角色**：primary 先以 `dmserver dm.ini mount` 启动，通过 disql 执行 `sp_set_oguid` 与 `ALTER DATABASE PRIMARY`；随后并发对 standby 执行相同流程（角色改为 `STANDBY`）
9. **启动守护进程与监视器**：各节点启动 `dmwatcher`（自动将 Mount 状态的实例切换为 Open），并在第一个 standby 节点启动 `dmmonitor` 确认监视器（集群无 standby 时 fallback 到 primary；监视器本身可以共置于任意节点，与该节点自身的 `sync_mode` 无关）；`dmmonitor.ini` 的仲裁列表（`MON_DW_IP`）只包含 `GLOBAL` 类型节点（primary + realtime 备库），sync/async 备库不参与
10. **配置备份作业 / 开启 SQL 日志 / 应用参数优化**：在每个节点上创建全备/增量备份作业、开启 SVR_LOG、执行官方自动参数调整脚本（调整后以 Mount 模式重启 dmserver 生效）

### 断点续传

集群部署同样支持断点续传。中断后直接重跑：

```sh
dm_installer install
```

工具会从中断的步骤继续，已完成的节点不会重复处理。第 8 步（启动数据库）中 primary 先于 standby 的顺序约束始终保留，不做更细粒度的拆分续传。

---

## 读写分离集群（开发中）

```sh
dm_installer init rws
```

生成 `config.toml`（`type = "rws"`）和 `rws.toml` 模板。

当前版本**仅生成配置模板**，部署逻辑尚未实现。`rws.toml` 结构与 `dw.toml` 相同，备节点需额外设置 `read_only = true`：

```toml
[[nodes]]
role     = "standby"
read_only = true
# ... 其余字段同 dw.toml
```

---

## 共享存储集群 DSC（开发中）

```sh
dm_installer init dsc
```

生成 `config.toml`（`type = "dsc"`）和 `dsc.toml` 模板。

当前版本**仅生成配置模板**，部署逻辑尚未实现。`dsc.toml` 在 `dw.toml` 基础上增加顶层字段 `shared_storage`（SAN 裸设备或 NFS 挂载点路径）：

```toml
oguid          = 453331
shared_storage = "/dev/sdc"

[[nodes]]
# ... 同 dw.toml
```
