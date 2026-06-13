---
quick_id: 260613-v8r
slug: ssh-standalone-toml-ssh-target-host-ssh
description: 单机安装支持 SSH 远程目标
---

# 单机安装 SSH 远程目标

## 目标
standalone.toml 新增可选 [ssh_target] 块，只支持密码认证。
host 为本机时跳过 SSH 直接本地安装，否则 SSH 远程执行。

## 变更文件

### 1. src/config/ssh.rs
新增 SshTarget struct：host、ssh_port（默认22）、user、password（Optional，None=运行时提示）

### 2. src/config/mod.rs
- InstallConfig 新增 ssh_target: Option<SshTarget>
- Default::default() 补 ssh_target: None
- validate_install_config 检查 ssh_target.user 非空

### 3. src/standalone/remote.rs（新建）
SSH 远程安装入口：
- 若 password 为 None，运行时提示输入 SSH 密码
- SshSession::connect 建立连接
- 幂等检测：exec "test -f {install_path}/dm.ini"
- 提示 DM 密码
- 本地下载/校验安装包
- 本地提取 DMInstall.bin
- SFTP 上传 DMInstall.bin + XML，远端静默安装
- 远端执行 dminit（含密码参数）

### 4. src/standalone/mod.rs
- mod remote 声明
- is_local_host(host) 判断：localhost/127.0.0.1/::1 + 系统 hostname
- run() 按 ssh_target 分支：本机→本地流程，远端→remote::run()

### 5. src/config/init.rs
STANDALONE_SPECIFIC 末尾追加注释掉的 [ssh_target] 示例块

## 约束
- 不实现 identity_file（本次只做密码认证）
- 复用 common/ssh::SshSession 和 CommandRunner trait
- shell_quote 所有路径参数防注入（同 deploy.rs 模式）
