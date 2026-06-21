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

必须填写本地安装包路径（集群模式不自动下载，工具会将此文件推送到各节点）：

```toml
type = "dw"

installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

log_level = "info"
```

**第三步：编辑 `dw.toml`**

填写节点信息（参考[配置参考 — dw.toml](configuration.md#dw-toml)中的字段说明）：

```toml
oguid = 453331

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

**第五步：部署**

```sh
dm_installer install
```

### 部署流程

工具按顺序自动完成以下步骤：

1. **预检**：并行检查所有节点的磁盘空间、端口占用、sudo 权限
2. **上传安装包**：逐节点 SFTP 上传（带进度条）
3. **静默安装**：在各节点执行 `DMInstall.bin` 静默安装 DM8
4. **dminit 初始化**：在各节点执行 `dminit` 初始化数据库实例
5. **分发配置文件**：
   - `dm.ini`（含 MAL_INI、ARCH_INI 等守护相关参数）
   - `dmmal.ini`（MAL 通信列表，各节点相同）
   - `dmarch.ini`（归档配置，主备内容不同）
   - `dmwatcher.ini`（数据守护配置）
6. **启动数据库**：先启动 primary，等待其上线后启动 standby
7. **启动守护进程**：在各节点启动 `dmwatcher`

### 断点续传

集群部署同样支持断点续传。中断后直接重跑：

```sh
dm_installer install
```

工具会从中断的步骤继续，已完成的节点不会重复处理。

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
