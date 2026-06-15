---
phase: 05-rws
fixed_at: 2026-06-14T11:30:00Z
review_path: .planning/phases/05-rws/05-REVIEW.md
iteration: 1
findings_in_scope: 10
fixed: 10
skipped: 0
status: all_fixed
---

# Phase 05: Code Review Fix Report

**Fixed at:** 2026-06-14T11:30:00Z
**Source review:** .planning/phases/05-rws/05-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 10
- Fixed: 10
- Skipped: 0

## Fixed Issues

### CR-01: 只读备库从未被打开——RWS 核心功能永远失效

**Files modified:** `src/cluster/phases.rs`, `src/cluster/rws/mod.rs`
**Commit:** a0fedec
**Applied fix:** 在 `run_read_routing_phase` 的轮询等待之前，对每个 `read_only=true` 的备节点调用 `deploy::configure_read_only_standby`。同时移除了死参数 `specific`（IN-02 合并修复）并更新测试以提供 `alter database open read only` 命令响应。

---

### CR-02: 集群节点 SSH 端口硬编码为 22，无法配置

**Files modified:** `src/config/ssh.rs`, `src/cluster/rws/mod.rs`, `src/cluster/primary_standby/mod.rs`, `src/config/validate.rs`, `src/standalone/remote.rs`, `src/cluster/deploy.rs`, `src/cluster/phases.rs`
**Commit:** 90be389
**Applied fix:** 在 `SshCredentials` 结构体中新增 `port: u16` 字段，使用 `#[serde(default = "default_ssh_port")]` 默认值为 22。所有 SSH 连接调用处改为使用 `node.ssh.port`，错误消息中也包含端口号以便诊断。同步更新了所有直接构造 `SshCredentials` 的测试代码。

---

### CR-03: SYSDBA 明文密码硬编码写入 shell 命令——密码修改后集群部署失效

**Files modified:** `src/config/cluster.rs`, `src/cluster/deploy.rs`, `src/cluster/phases.rs`
**Commit:** 23d884c
**Applied fix:** 在 `DminitConfig` 中新增 `sysdba_password: String` 字段（`#[serde(default = "default_sysdba_password")]`，默认 "SYSDBA"）。替换 deploy.rs 中的 5 处和 phases.rs 中的 1 处 `SYSDBA/SYSDBA@localhost` 硬编码为 `SYSDBA/{shell_quote(sysdba_password)}@localhost`。

---

### WR-01: `run_sqllog_phase` 对只读备库执行写操作，必然失败

**Files modified:** `src/cluster/phases.rs`
**Commit:** be36571
**Applied fix:** 在 `run_sqllog_phase` 的 `.iter()` 后添加 `.filter(|(node, _)| !node.read_only)` 过滤，跳过 `read_only=true` 的只读备库节点。

---

### WR-02: `checkpoint.rs::load_from` 存在 TOCTOU 竞态

**Files modified:** `src/cluster/checkpoint.rs`
**Commit:** 0d1ffa3
**Applied fix:** 将 `if !path.exists() { return Ok(None); }` + `read_to_string(&path)?` 两步操作替换为单次 `match std::fs::read_to_string(&path)` 调用，将 `ErrorKind::NotFound` 映射为 `Ok(None)`，消除竞态窗口。

---

### WR-03: `run_verify_phase` 在只读备库打开前执行，验证结论不可信

**Files modified:** `src/cluster/rws/mod.rs`, `src/cluster/deploy.rs`
**Commit:** 3864ccf
**Applied fix:** 在 `rws/mod.rs` 的 `run_with_runners` 中将 `run_read_routing_phase` 移到 `run_verify_phase` 之前执行。同时在 `verify_node_role`（deploy.rs）中对 `read_only=true` 的备节点新增 `STATUS$=OPEN` 断言检查，令验证结果真实可信。

---

### WR-04: `wait_for_standby_open_impl` 最后一次轮询不打印警告日志

**Files modified:** `src/cluster/phases.rs`
**Commit:** 03ba275
**Applied fix:** 将 `tracing::warn!` 移出 `if attempt < max_retries` 条件块，使最后一次（`attempt == max_retries`）失败时也会记录警告日志，操作员可看到完整的重试历史。

---

### IN-01: `checkpoint.rs::load_from` 混用 `println!` 与 tracing

**Files modified:** `src/cluster/checkpoint.rs`
**Commit:** 0d1ffa3
**Applied fix:** 将 `println!("[续] 检测到检查点，从上次进度继续安装")` 替换为 `tracing::info!("[续] 检测到检查点，从上次进度继续安装")`（与 WR-02 合并在同一提交中）。

---

### IN-02: `run_read_routing_phase` 的 `specific` 参数显式丢弃

**Files modified:** `src/cluster/phases.rs`, `src/cluster/rws/mod.rs`
**Commit:** a0fedec
**Applied fix:** 从 `run_read_routing_phase` 函数签名中移除 `specific: &ClusterSpecificConfig` 参数（与 CR-01 合并在同一提交中），同步更新 rws/mod.rs 调用点和 phases.rs 测试用例。

---

### IN-03: `default_oguid` 将今日日期硬编码为编译期常量

**Files modified:** `src/config/cluster.rs`
**Commit:** a1a99b1
**Applied fix:** 从 `ClusterSpecificConfig.oguid` 字段移除 `#[serde(default = "default_oguid")]` 属性，删除 `default_oguid()` 函数，使 oguid 成为必填字段。更新所有缺少 oguid 字段的测试 TOML（6 处），将 `test_default_oguid_is_today` 替换为 `test_missing_oguid_fails` 验证新约束。

---

_Fixed: 2026-06-14T11:30:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
