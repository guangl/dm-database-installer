---
quick_id: 260615-tzh
slug: refactor-standalone-cluster-code-reuse
description: 重构单机安装与集群安装代码复用——添加 LocalRunner，统一 preflight 和 env_setup 路径
date: 2026-06-15
status: in-progress
---

# Quick Task: 重构单机/集群代码复用

## Goal

单机安装的 preflight 和 env_setup 目前有独立的本地实现，与集群路径的 `CommandRunner`-based 实现重复约 300+ 行。通过添加 `LocalRunner` 并统一入口消除重复。

## Must-Haves

- [ ] `src/common/ssh/local.rs` — 实现 `LocalRunner`，满足 `CommandRunner` trait
- [ ] `src/standalone/env_setup.rs` — 删除 `run_local()` 及所有 local-only 函数，统一为 `run(runner: &dyn CommandRunner)`
- [ ] `src/standalone/mod.rs` — 删除 `check_local_prerequisites` 及其 6 个辅助函数，改用 `cluster::preflight` 的通用检查 + `LocalRunner`
- [ ] `src/common/ssh/mod.rs` — 导出 `LocalRunner`
- [ ] `cargo build` 通过，`cargo test` 通过

## Tasks

### Task 1: 添加 LocalRunner

**文件**: `src/common/ssh/local.rs`

实现 `CommandRunner` trait：

```rust
pub struct LocalRunner;

#[async_trait]
impl CommandRunner for LocalRunner {
    async fn exec(&self, command: &str) -> Result<(Vec<u8>, u32), SshError> {
        // 用 tokio::process::Command 执行 sh -c command
        // 正确传递 exit_code（非零不变成 Err，而是 Ok((stdout, exit_code))）
        // stderr 忽略或合并到 stdout
    }

    async fn sftp_write(&self, remote_path: &str, bytes: &[u8]) -> Result<(), SshError> {
        // tokio::fs::write(remote_path, bytes)
    }

    async fn sftp_read(&self, remote_path: &str) -> Result<Vec<u8>, SshError> {
        // tokio::fs::read(remote_path)
    }
}
```

**关键**：exec 返回 `Ok((stdout, exit_code))`，exit_code 可以非零。只有命令本身无法执行（spawn 失败）时才返回 `Err(SshError::ExecFailed)`。

在 `src/common/ssh/mod.rs` 添加 `pub use local::LocalRunner;`

### Task 2: 重构 env_setup.rs

**文件**: `src/standalone/env_setup.rs`

1. 将 `run_remote(runner: &dyn CommandRunner)` 重命名为 `run(runner: &dyn CommandRunner)`
2. 删除 `run_local()` 和所有 local-only 函数（约 250 行）：
   - `detect_local_privilege`, `is_root`, `can_sudo`
   - `setup_dmdba_user`, `disable_selinux`, `disable_transparent_hugepages`
   - `set_timezone`, `set_locale`, `optimize_sshd`, `disable_firewall`
   - `configure_limits`, `configure_pam`, `configure_sysctl`
   - `verify_sysctl_applied`, `compute_shm`
   - `cmd_succeeds`, `run_priv`, `pipe_to_priv`, `append_priv`
3. 保留所有 remote_* 函数（重命名为去掉 `remote_` 前缀）
4. **关键**：`remote_configure_sysctl` 里 shmall/shmmax 是 shell 计算的，改为 Rust 计算后通过 `sftp_write` 写入 `/etc/sysctl.conf`。具体：
   - 先 `sftp_read("/proc/meminfo")` 读取内存
   - Rust 计算 shmall/shmmax（同原来的 `compute_shm()`）
   - 用 `sftp_write` 追加到 `/etc/sysctl.conf`

5. 更新 `src/standalone/mod.rs` 中的调用：
   ```rust
   // 之前：
   env_setup::run_local()?;
   // 之后：
   let local = crate::common::ssh::LocalRunner;
   env_setup::run(&local).await?;
   ```
   注意：`run()` 现在是 async，`run` 函数里的调用也需要 await。

### Task 3: 重构 standalone preflight

**文件**: `src/standalone/mod.rs`

1. 删除 `check_local_prerequisites` 函数及其 6 个辅助函数（约 130 行）：
   - `check_local_port`, `check_local_ulimits`, `check_local_selinux`
   - `check_local_disk`, `check_local_memory`, `check_local_cpu`, `parse_df_bytes`

2. 提取 `cluster::preflight` 里的通用检查为公共函数（standalone 只需要通用的，不需要集群专有的 mal_port/dw_port/time_sync/sudo/dmdba_user/kernel_params/inter_node_connectivity 检查）

   通用检查：`check_port_available`, `check_disk_space`, `check_memory`, `check_cpu_cores`, `check_ulimits`, `check_selinux`

3. 在 standalone 中调用：
   ```rust
   async fn check_standalone_prerequisites(install_path: &str, port: u16) -> Result<()> {
       let runner = LocalRunner;
       cluster::preflight::check_port_available(&runner, port).await?;
       cluster::preflight::check_disk_space(&runner, install_path).await?;
       cluster::preflight::check_memory(&runner).await?;
       cluster::preflight::check_cpu_cores(&runner).await?;
       cluster::preflight::check_ulimits(&runner).await?;
       cluster::preflight::check_selinux(&runner).await?;
       Ok(())
   }
   ```
   
   注意：`cluster::preflight` 里这些函数是 `pub async fn`，但 `check_node` 是全集群专用的，standalone 不用它。

4. 更新 `run()` 入口：
   - `check_local_prerequisites` 改为 `check_standalone_prerequisites(&specific.install_path, specific.port).await?`
   - `env_setup::run_local()` 改为 `env_setup::run(&LocalRunner).await?`
   - 函数签名改为 `pub async fn run(...)` 已经是 async，ok
   - `check_local_prerequisites` 是 sync 的，改成 async 后需要 await

## Non-Goals

- 不改 rollback.rs（保持现有 RAII Drop 结构）
- 不改集群路径（cluster/preflight.rs、cluster/deploy.rs 等）
- 不改 standalone/remote.rs

## Execution Notes

- 改动后先 `cargo build` 确认编译通过
- 再 `cargo test` 确认测试通过（特别是 cluster/preflight 的 tests）
- `src/standalone/env_setup.rs` 里 `remote_check_privilege` 已经有 `run_remote` 调用 `remote_read_str`，重构时连同一起处理
- `src/standalone/remote.rs` 里可能调用 `env_setup::run_remote`，需要同步改为 `env_setup::run`
