---
phase: "04-release"
plan: "01"
subsystem: "cluster-ssh-deploy"
tags:
  - rust
  - security
  - bug-fix
  - sftp
  - shell-injection

dependency_graph:
  requires: []
  provides:
    - "ssh.rs: expand_tilde + sftp.create 修复 + TOFU 指纹日志"
    - "deploy.rs: shell_quote + .bin 上传执行链路"
    - "Cargo.toml: cargo-dist 必填 metadata"
  affects:
    - "04-02: cargo dist init 依赖本 plan 的 Cargo.toml metadata"

tech_stack:
  added: []
  patterns:
    - "shell_quote 单引号转义防注入（类比 xml_escape 模式）"
    - "expand_tilde 路径工具函数（同 xml_escape 位置风格）"
    - "sftp.create() + AsyncWriteExt::write_all 替代 sftp.write()"
    - "tracing::warn! 记录安全敏感操作"

key_files:
  created: []
  modified:
    - path: "src/cluster/ssh.rs"
      changes: "expand_tilde 函数 + sftp.create 修复 + TOFU 指纹 warn + mutex 无 unwrap"
    - path: "src/cluster/deploy.rs"
      changes: "shell_quote 函数 + .bin 上传链路 + chmod +x + 全部 user-controlled 字段转义"
    - path: "Cargo.toml"
      changes: "新增 description / license / repository 三字段"

decisions:
  - "shell_quote 使用单引号转义（而非双引号）：单引号转义更安全，'$()' 等展开在单引号内完全失效"
  - "expand_tilde 保持 HOME 未设置时无 panic 返回原路径：符合最小惊讶原则"
  - "sftp.create() 替换 sftp.write()：SFTP write flag 无 CREATE 标志，无法创建不存在的远端文件"
  - "test_build_dminit_args_format 随 CR-04 修复同步更新：该测试现在验证 shell_quote 包裹后的路径格式"
  - "未实现可选的 validate_safe_path（config 层白名单）：deploy.rs 内的 shell_quote 已是充分第一道防线"

metrics:
  duration: "~40 minutes"
  completed_date: "2026-06-13T12:11:22Z"
  tasks_completed: 3
  files_changed: 3
  tests_added: 9
  tests_modified: 1
---

# Phase 04 Plan 01: Critical Bug Fixes + Cargo.toml Metadata Summary

**一句话：** 修复 5 个阻止集群部署的 Critical bug（SFTP CREATE 标志、.bin 上传路径、tilde 展开、shell 注入、TOFU 指纹），并补齐 cargo-dist init 所需的 Cargo.toml 必填字段。

## 任务完成状态

| Task | 名称 | 状态 | 提交 |
|------|------|------|------|
| 1 | 修复 ssh.rs (CR-02/CR-03/CR-05) | 完成 | 5b03c1f (RED), 5e3fa8f (GREEN) |
| 2 | 修复 deploy.rs (CR-01/CR-04) | 完成 | 424ea7b (RED), 91a4045 (GREEN) |
| 3 | 补齐 Cargo.toml metadata | 完成 | d58ecee |

## CR-XX Bug 修复详情

### CR-02: SFTP CREATE 标志缺失（ssh.rs: sftp_write）

**修复位置：** `src/cluster/ssh.rs` 第 191-207 行

**问题：** `sftp.write(remote_path, bytes)` 调用的 SFTP 写入没有 CREATE 标志，无法创建不存在的远端文件，导致 `distribute_configs` 100% 失败。

**修复：** 替换为 `sftp.create(remote_path).await?` + `remote_file.write_all(bytes).await`，正确创建新文件后写入。新增 `use tokio::io::AsyncWriteExt;`。

### CR-03: tilde 路径未展开（ssh.rs: try_key_auth）

**修复位置：** `src/cluster/ssh.rs` 第 125-139 行（expand_tilde 函数）+ 第 147 行（调用点）

**问题：** `~/.ssh/id_rsa` 对 Rust `PathBuf` 是字面字符，`load_secret_key` 找不到文件，silently fallback 到密码认证。

**修复：** 在 `try_key_auth` 前添加 `fn expand_tilde(path: &PathBuf) -> PathBuf`，通过 `strip_prefix("~/")` + `std::env::var_os("HOME")` 展开。HOME 未设置时原路径返回，不 panic。

### CR-05: TOFU 静默接受公钥（ssh.rs: check_server_key）

**修复位置：** `src/cluster/ssh.rs` 第 51-65 行

**问题：** `TofuHandler::check_server_key` 无条件接受任意服务器公钥，无日志，运维无法比对指纹。同时使用了 `unwrap()` 操作 Mutex。

**修复：** 计算 `server_public_key.fingerprint(Default::default())`，通过 `tracing::warn!("[ssh][TOFU] ...")` 记录。Mutex 操作改为 `match self.accepted_keys.lock()` 处理 poison 情况，移除 `unwrap()`。

### CR-01: 安装包 .iso 路径 + PATH 依赖（deploy.rs: upload_installer_and_install）

**修复位置：** `src/cluster/deploy.rs` 第 57-69 行

**问题：** 上传的文件变量名为 `remote_iso`，后缀 `.iso`，且 `install_cmd` 使用 `cd /tmp && DMInstall.bin`（依赖远端 PATH，通常不存在）。

**修复：** 变量改为 `remote_bin_path`（`.bin` 后缀），上传后新增 `chmod +x` 步骤，`install_cmd` 改为按完整路径执行 `shell_quote(&remote_bin_path) -q ...`。

### CR-04: shell 命令注入（deploy.rs: 多处）

**修复位置：** `src/cluster/deploy.rs` 第 14-16 行（shell_quote 函数）+ 多处调用

**问题：** `install_path`、`data_path`、`instance_name` 均为用户可控字段，直接拼入 `format!()` 生成 shell 命令，可通过特殊字符执行任意命令。

**修复：** 新增 `fn shell_quote(raw: &str) -> String`（单引号包裹 + `'` 转义为 `'\''`），在以下函数中所有 user-controlled 字段调用前应用：
- `build_dminit_args`：install_path、data_path、instance_name（3 处）
- `upload_installer_and_install`：remote_bin_path、remote_xml（2 处）
- `distribute_configs`：target_path 两侧（2 处）
- `start_dmserver_mount`：install_path、data_path、instance_name、log_path（4 处，含 let 绑定）
- `configure_database_role`：install_path（1 处）
- `start_dmwatcher`：install_path、data_path、instance_name、log_path（4 处，含 let 绑定）

## 新增测试列表

### Task 1 新增（ssh.rs）

| 测试名 | 验证内容 |
|--------|---------|
| `test_expand_tilde_replaces_home` | `~/` 展开为 `$HOME` 路径 |
| `test_expand_tilde_no_tilde_unchanged` | 绝对路径原样返回 |
| `test_expand_tilde_missing_home_returns_input` | HOME 未设置时不 panic |
| `test_tofu_logs_fingerprint` | TOFU handler 输出含 `[ssh][TOFU]` 日志 |

### Task 2 新增（deploy.rs）

| 测试名 | 验证内容 |
|--------|---------|
| `test_shell_quote_single_quotes_path` | 正常路径被单引号包裹 |
| `test_shell_quote_escapes_embedded_single_quote` | 内嵌单引号正确转义为 `'\''` |
| `test_shell_quote_blocks_injection` | 注入字符被单引号保护 |
| `test_start_dmserver_mount_quotes_paths` | install_path 经 shell_quote 出现在命令中 |

### Task 2 修改（deploy.rs）

| 测试名 | 修改内容 |
|--------|---------|
| `test_upload_installer_and_install_pushes_xml` | 验证 `.bin` 路径而非 `.iso` + `chmod +x` 调用 |
| `test_build_dminit_args_format` | 期望 shell_quote 包裹后的路径格式（Rule 1 auto-fix） |

## Cargo.toml 新增字段

```toml
description = "达梦数据库安装器 (dm-database-installer) — 单机/集群静默部署工具"
license = "MIT"
repository = "https://github.com/guangl/dm-database-installer"
```

## 给 Plan 02 的接口约定

- `Cargo.toml` 的 `description`、`license`、`repository` 三个字段已就绪，`cargo dist init` 可直接运行
- `[workspace.metadata.dist]` 和 `[profile.dist]` 块由 Plan 02 的 `cargo dist init` 命令生成，本 plan 未添加

## 可选加固状态

**`validate_safe_path`（config 层路径白名单）：未实现**

决策依据：`deploy.rs` 内的 `shell_quote` 已在第一道防线（执行层）完整覆盖所有 user-controlled 字段，config 层白名单属于额外纵深防御。在不增加 `src/config/mod.rs` 改动复杂度的前提下，当前修复已满足 plan 的必做要求。若未来引入 config 层验证，可在独立 plan 中添加。

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] 更新 test_build_dminit_args_format 适配 shell_quote 输出**

- **发现于：** Task 2 GREEN 阶段运行测试时
- **问题：** `build_dminit_args` 在 CR-04 修复后返回 `"'/opt/dmdbms'/bin/dminit"` 等 shell_quote 包裹路径，但原测试期望 `"/opt/dmdbms/bin/dminit"`（未包裹格式）
- **修复：** 更新测试断言以匹配修复后的正确行为，新的断言同时验证 shell_quote 包裹已生效
- **修改文件：** `src/cluster/deploy.rs`（tests 模块内 `test_build_dminit_args_format`）
- **提交：** 91a4045（含于 GREEN 阶段提交）

## Known Stubs

无。所有修复均为实际功能修复，无 placeholder 或硬编码空值。

## Self-Check: PASSED

- src/cluster/ssh.rs 存在：已确认（含 expand_tilde、sftp.create、TOFU warn）
- src/cluster/deploy.rs 存在：已确认（含 shell_quote、.bin 路径、chmod +x）
- Cargo.toml 新增字段：已确认（description/license/repository 各 1 行）
- 所有提交存在：5b03c1f, 5e3fa8f, 424ea7b, 91a4045, d58ecee
- cargo test 全绿：87 passed; 0 failed
- cargo build --release 通过
