# 集群部署

## 支持的集群类型

| 类型 | 命令 | 状态 |
|------|------|------|
| 主备（Primary-Standby） | `dm-installer init cluster primary-standby` | ✅ 已支持 |
| 读写分离（RWS） | `dm-installer init cluster rws` | 🚧 开发中 |
| 共享存储（DSC） | `dm-installer init cluster dsc` | 🚧 开发中 |

---

## 主备集群（Primary-Standby）

### 前提条件

- 控制机能通过 SSH 访问所有节点
- 提前准备好 DM8 安装包（`.iso` 文件）
- 各节点已安装 `unzip`

### 步骤

```sh
# 1. 生成配置模板
dm-installer init cluster primary-standby

# 生成的文件：
#   config.toml             — 通用配置
#   primary-standby.toml   — 主备特有配置
```

**`config.toml`**

```toml
type = "primary-standby"

# 集群模式必须指定本地安装包路径
installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

log_level = "info"
```

**`primary-standby.toml`**（填写节点信息）

```toml
# 数据守护组名称
group_name = "GRP1"
# 唯一标识，同一数据守护系统内所有实例必须相同
oguid = "45331"

[[nodes]]
role    = "primary"
host    = "192.168.1.101"
port    = 5236
ssh_port = 22
user    = "root"
# password 不填则运行时提示

[[nodes]]
role    = "standby"
host    = "192.168.1.102"
port    = 5236
ssh_port = 22
user    = "root"
```

```sh
# 2. 验证配置
dm-installer validate

# 3. 开始部署（控制机需能访问所有节点）
dm-installer install
```

### 部署流程

工具按顺序自动完成以下步骤：

1. 逐节点 SFTP 上传安装包（带进度条）
2. 静默安装 DM8
3. 生成并分发 `dm.ini`（含 MAL_INI、ARCH_INI 等参数）
4. 生成并分发 `dmmal.ini`（MAL 通信列表）
5. 主节点生成 `dmarch.ini`（归档配置）
6. 备节点生成 `dmarch.ini`
7. 生成 `dmwatcher.ini`（数据守护配置）
8. 启动数据守护

### 断点续传

集群部署同样支持断点续传。中断后重跑 `dm-installer install` 即可从中断步骤继续，已完成的节点不会重复处理。

---

## 读写分离集群（开发中）

```sh
dm-installer init cluster rws
```

当前版本仅生成配置模板，部署逻辑尚未实现。

---

## 共享存储集群 DSC（开发中）

```sh
dm-installer init cluster dsc
```

当前版本仅生成配置模板，部署逻辑尚未实现。
