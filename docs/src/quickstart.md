# 快速开始

## 一行命令（最快路径）

适合开发者在本地快速拉起 DM8 环境，无需安装任何工具：

```sh
curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh | bash
```

脚本自动检测系统架构与发行版，下载并静默安装 DM8。安装完成后会输出随机生成的 SYSDBA / SYSAUDITOR 密码，请妥善保存。

> 仅支持 Linux（x86_64 / aarch64）。需要自定义参数或生产部署，请使用下方 `dm-installer` 工具。

---

## 使用 dm-installer 工具（自定义参数）

首先按[安装说明](installation.md)安装 `dm-installer`，然后：

### 本地单机安装

```sh
# 1. 生成配置模板（当前目录）
dm-installer init standalone

# 2. 检查 standalone.toml，默认值通常够用，端口 5236 可按需修改
# cat standalone.toml

# 3. 启动安装（自动下载适配当前系统的 DM8 安装包）
dm-installer install
```

安装完成后，工具会输出随机生成的 SYSDBA / SYSAUDITOR 密码，请妥善保存。

## SSH 远程安装

适用于在控制机上远程部署到目标服务器（无需手动登录目标机器）。

```sh
dm-installer init standalone
```

编辑 `standalone.toml`，填写 SSH 目标（取消注释 `[ssh_target]` 部分）：

```toml
[install]
install_path = "/home/dmdba/dmdbms"
data_path    = "/home/dmdba/dmdbms/data"

[instance]
port = 5236

[ssh_target]
host         = "192.168.1.100"   # 目标服务器 IP
ssh_port     = 22
user         = "root"
# password 不填则运行时提示输入（推荐）
max_retries  = 3
```

```sh
dm-installer install
```

工具会自动：
1. 下载适配**目标服务器**平台的安装包
2. 通过 SFTP 上传到目标机器（带进度条）
3. 在目标机器上静默安装

## 主备集群安装

```sh
# 1. 生成配置模板
dm-installer init cluster primary-standby

# 2. 编辑 config.toml：指定本地安装包路径（集群模式必须提前准备）
# installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

# 3. 编辑 primary-standby.toml：填写各节点 IP、SSH 凭证、OGUID 等

# 4. 验证配置
dm-installer validate

# 5. 部署
dm-installer install
```

## 断点续传

如果安装中途因网络或其他原因中断，再次执行 `dm-installer install` 时会自动检测检查点文件，跳过已完成的步骤从中断处继续。

强制重新开始（忽略检查点）：

```sh
rm dm_installer_checkpoint.json
dm-installer install
```

## 验证配置

在实际安装前验证配置文件语法和语义合法性：

```sh
dm-installer validate
# 或指定其他路径
dm-installer validate --config /path/to/config.toml
```
