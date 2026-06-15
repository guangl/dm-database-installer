---
phase: 07-dsc
fixed_at: 2026-06-15T07:30:00Z
review_path: .planning/phases/07-dsc/07-REVIEW.md
iteration: 1
findings_in_scope: 11
fixed: 11
skipped: 0
status: all_fixed
---

# Phase 07-dsc: Code Review Fix Report

**Fixed at:** 2026-06-15T07:30:00Z
**Source review:** .planning/phases/07-dsc/07-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 11
- Fixed: 11
- Skipped: 0

## Fixed Issues

### CR-01: verify_dsc_node 始终使用 dminit.port（5236），无法验证非主节点

**Files modified:** `src/cluster/dsc/deploy.rs`, `src/cluster/dsc/mod.rs`
**Commit:** 85ded3b
**Applied fix:** 为 `verify_dsc_node` 增加 `node_port: u16` 参数，函数内使用 `node_port` 而非 `dminit.port`；在 `mod.rs` 的 `verify_all_nodes_dmserver` 中计算 `dminit.port.saturating_add(node_idx as u16)` 并传入。测试调用侧传入 `dminit.port`（node 0 端口，测试仅有单节点场景）。

---

### CR-02: DMASM 健康检查始终轮询端口 9349，忽略各节点实际端口

**Files modified:** `src/cluster/dsc/mod.rs`
**Commit:** 5559c2c
**Applied fix:** 在 `run_start_css_asm_all_nodes` 的健康检查循环中，改用 `runners.iter().enumerate()` 并计算 `asm_port = 9349u16 + (node_idx as u16) * 2`，与 `dmdcr_cfg.ini` 中 ASM 端口分配保持一致。

---

### CR-03: first_node（Primary）的 dmserver 健康检查端口错误

**Files modified:** `src/cluster/dsc/mod.rs`
**Commit:** 0b4d085
**Applied fix:** 在 `start_first_node_dmserver` 中计算 `first_node_port = dminit.port.saturating_add(first_idx as u16)`，替换原来的固定 `dminit.port`。

---

### WR-01: 三个函数超出 40 行限制（违反项目约定）

**Files modified:** `src/cluster/dsc/mod.rs`
**Commit:** 662a6f8
**Applied fix:**
- `run_with_runners`（原 95 行）：提取 `run_early_gates`（Gate 1-4）和 `run_later_gates`（Gate 5-8）两个私有 helper，主函数精简为 ~15 行
- `run_start_and_verify_dmserver_all_nodes`（原 64 行）：提取 `start_first_node_dmserver`、`start_other_nodes_dmserver`、`verify_all_nodes_dmserver` 三个私有 helper，主函数精简为 ~8 行

---

### WR-02: distribute_config_dir 的 first_node_index 参数未实际使用

**Files modified:** `src/cluster/dsc/deploy.rs`, `src/cluster/dsc/mod.rs`
**Commit:** 597aa72
**Applied fix:** 移除 `distribute_config_dir` 签名中的 `first_node_index: usize` 参数及函数末尾的 `let _ = first_node_index;`；更新 `mod.rs` 调用处（去掉 `first_idx` 实参）和 `deploy.rs` 测试调用处（去掉 `0` 实参）。

---

### WR-03: first_node_index 不必要地声明为 async

**Files modified:** `src/cluster/dsc/mod.rs`
**Commit:** ce26ce1
**Applied fix:** 将 `first_node_index` 函数签名从 `async fn` 改为 `fn`；将所有调用处的 `.await?` 改为 `?`（共 4 处）。

---

### WR-04: 测试的 set_current_dir 恢复不具备 panic 安全性

**Files modified:** `src/cluster/dsc/mod.rs`
**Commit:** 2491ddb
**Applied fix:** 在 `#[cfg(test)] mod tests` 中添加 `CwdGuard` RAII 结构体（`struct CwdGuard(std::path::PathBuf)` + `impl Drop`），在 5 个使用 `set_current_dir` 的测试中将手动恢复模式改为 `let _cwd_guard = CwdGuard(std::env::current_dir().unwrap())`，并删除测试末尾的手动 `set_current_dir(original_dir)` 调用。

---

### WR-05: DSC 验证未要求最少 2 个节点

**Files modified:** `src/config/cluster.rs`
**Commit:** 364fb2a（与 IN-03 合并提交）
**Applied fix:** 在 `validate_dsc` 中添加 `non_monitor_count < 2` 检查，返回错误消息"DSC 集群至少需要 2 个节点（1 primary + 1 standby）"。

---

### IN-01: #![allow(dead_code)] 在 Plan 04 完成后应移除

**Files modified:** `src/cluster/dsc/deploy.rs`, `src/cluster/dsc/templates.rs`
**Commit:** 7f921e2
**Applied fix:** 删除 `deploy.rs` 第 1-2 行和 `templates.rs` 第 1-2 行的 `// 注释` + `#![allow(dead_code)]`。`cargo build` 无警告确认所有函数已被引用。

---

### IN-02: dminit.ini 中 SYSDBA_PWD 以明文写入远程磁盘

**Files modified:** `src/cluster/dsc/deploy.rs`
**Commit:** a106b6f
**Applied fix:** 在 `run_dminit_shared` 的 dminit 执行成功后，添加 `rm -f` 命令删除远程 `dminit.ini`；若删除失败则以 `tracing::warn!` 记录警告，不中断流程。

---

### IN-03: DSC 配置允许 Monitor 角色节点但部署逻辑忽略它

**Files modified:** `src/config/cluster.rs`
**Commit:** 364fb2a（与 WR-05 合并提交）
**Applied fix:** 在 `validate_dsc` 中添加 `has_monitor` 检查，若存在 Monitor 角色节点则返回错误"DSC 集群不支持 monitor 角色节点，请仅配置 primary/standby"。

---

## 编译与测试验证

`cargo build` — 编译成功，无警告
`cargo test` — 246 个测试全部通过

---

_Fixed: 2026-06-15T07:30:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
