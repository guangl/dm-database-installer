# 配置参考

dm-installer 使用两个配置文件（均由 `dm-installer init` 生成模板）：

| 文件 | 说明 |
|------|------|
| `config.toml` | 通用配置：安装类型、安装包来源、日志级别 |
| `standalone.toml` / `primary-standby.toml` / … | 各类型特有配置 |

---

## config.toml

```toml
# 安装类型（必填）
# standalone          — 单机
# primary-standby     — 主备集群
# rws                 — 读写分离集群（配置模板已生成，部署逻辑开发中）
# dsc                 — 共享存储集群（同上）
type = "standalone"

# 本地安装包路径
# 单机：不填则自动下载匹配当前平台的版本
# 集群：必填（会被推送到各节点）
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

### [instance] — 实例参数

| 键 | 默认值 | 有效值 | 说明 |
|----|--------|--------|------|
| `instance_name` | `DMSERVER` | 任意字符串 | 实例名称 |
| `port` | `5236` | 1–65535 | 监听端口 |
| `page_size` | `32` | `4` / `8` / `16` / `32` | 页大小（KB） |
| `charset` | `1` | `0` / `1` / `2` | 0=GB18030 1=UTF-8 2=EUC-KR |
| `case_sensitive` | `true` | `true` / `false` | 标识符大小写敏感 |
| `extent_size` | `32` | `16` / `32` | 区段大小（页数） |

### [ssh_target] — SSH 远程目标（可选）

省略此节时在本机安装。

| 键 | 默认值 | 说明 |
|----|--------|------|
| `host` | — | 目标主机 IP 或域名 |
| `ssh_port` | `22` | SSH 端口 |
| `user` | — | SSH 用户名 |
| `password` | — | SSH 密码（不填则运行时提示输入） |
| `max_retries` | `3` | SSH 连接失败重试次数 |
| `retry_interval_secs` | `5` | 重试间隔（秒） |

**完整示例：**

```toml
[install]
install_path = "/home/dmdba/dmdbms"
data_path    = "/home/dmdba/dmdbms/data"

[instance]
instance_name  = "DMSERVER"
port           = 5236
page_size      = 32
charset        = 1
case_sensitive = true
extent_size    = 32

[ssh_target]
host               = "192.168.1.100"
ssh_port           = 22
user               = "root"
max_retries        = 3
retry_interval_secs = 5
```

---

## 环境变量

| 变量 | 说明 |
|------|------|
| `RUST_LOG` | 覆盖日志级别（如 `RUST_LOG=debug dm-installer install`） |
