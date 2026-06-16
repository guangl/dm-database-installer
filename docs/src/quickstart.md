# 快速开始

## 路径一：一行命令（最快）

适合开发者在本地或测试机快速拉起 DM8，无需安装任何工具：

```sh
curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh | bash
```

脚本自动检测系统架构与发行版，下载并静默安装 DM8，注册 systemd 服务，安装完成后打印 SYSDBA / SYSAUDITOR 密码。

需要自定义安装目录或端口时，通过环境变量传入即可：

```sh
DM_INSTALL_PATH=/opt/dmdbms DM_PORT=5237 bash -c \
  "$(curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh)"
```

> 仅支持 Linux（x86_64 / aarch64）。需要集群部署或更多精细配置，使用路径二。

---

## 路径二：dm-installer 工具

先按[安装说明](installation.md)安装 `dm-installer`。

### 本地单机安装

```sh
# 1. 在当前目录生成配置模板
dm-installer init standalone

# 生成的文件：
#   config.toml      — 通用配置（type = "standalone"）
#   standalone.toml  — 单机特有配置（端口、路径、字符集等）

# 2. 按需修改 standalone.toml（默认值通常够用，端口 5236 可改）
#    默认安装路径：/home/dmdba/dmdbms

# 3. 安装（自动下载适配当前系统的 DM8 安装包）
dm-installer install
```

安装完成后，工具会输出随机生成的 SYSDBA / SYSAUDITOR 密码，请妥善保存。

---

### SSH 远程安装

在控制机上部署到目标服务器，无需手动登录目标机器。

```sh
# 1. 生成配置模板
dm-installer init standalone
```

编辑 `standalone.toml`，取消注释 `[ssh_target]` 节并填写目标信息：

```toml
[install]
install_path = "/home/dmdba/dmdbms"
data_path    = "/home/dmdba/dmdbms/data"

[instance]
port = 5236

[ssh_target]
host     = "192.168.1.100"   # 目标服务器 IP 或域名
ssh_port = 22
user     = "root"
# password 不填则安装时提示输入（推荐，避免明文存储密码）
max_retries = 3
```

```sh
# 2. 安装
dm-installer install
```

工具会自动：

1. 检测**目标服务器**的 OS 和架构，下载对应安装包
2. 通过 SFTP 上传安装包（带进度条）
3. 在目标机器上静默安装并注册服务

---

### 主备集群安装（DW）

```sh
# 1. 生成配置模板
dm-installer init dw

# 生成的文件：
#   config.toml  — 通用配置（type = "dw"）
#   dw.toml      — 主备特有配置（节点列表、OGUID、端口等）

# 2. 编辑 config.toml：指定本地安装包路径（集群必须预先下载）
#    installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

# 3. 编辑 dw.toml：填写各节点信息（IP、SSH 凭证、OGUID 等）

# 4. 验证配置
dm-installer validate

# 5. 部署（控制机需能 SSH 访问所有节点）
dm-installer install
```

详细配置说明见[集群部署](cluster.md)。

---

## 验证配置

在实际安装前验证配置文件的语法和语义：

```sh
# 验证当前目录的 config.toml（及对应特有配置文件）
dm-installer validate

# 或指定路径
dm-installer validate --config /path/to/config.toml
```

常见语义检查项：`page_size` 枚举值（4/8/16/32）、`charset` 枚举值（0/1/2）、端口冲突、节点 SSH 凭证完整性、集群 OGUID 范围等。

---

## 断点续传

安装中途因网络或其他原因中断时，直接重跑即可：

```sh
dm-installer install
```

工具会检测当前目录的 `dm_installer_checkpoint.json`，跳过已完成的步骤，从中断处继续。

若需要**强制重新开始**（忽略检查点）：

```sh
rm dm_installer_checkpoint.json
dm-installer install
```
