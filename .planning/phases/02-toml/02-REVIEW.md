---
phase: 02-toml
reviewed: 2026-06-12T10:19:31Z
depth: standard
files_reviewed: 5
files_reviewed_list:
  - .claude/worktrees/agent-a693079c0c4cadfbf/src/config/mod.rs
  - .claude/worktrees/agent-a693079c0c4cadfbf/src/config/validate.rs
  - .claude/worktrees/agent-a693079c0c4cadfbf/src/install/mod.rs
  - .claude/worktrees/agent-a693079c0c4cadfbf/src/cli.rs
  - .claude/worktrees/agent-a693079c0c4cadfbf/tests/fixtures/semantic_invalid.toml
findings:
  critical: 0
  warning: 4
  info: 3
  total: 7
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-06-12T10:19:31Z
**Depth:** standard
**Files Reviewed:** 5
**Status:** issues_found

## Summary

本次审查涵盖 Phase 2 新增的 TOML 配置文件支持：`config/mod.rs`（加载与验证逻辑）、`config/validate.rs`（validate 子命令）、`install/mod.rs`（resolve_config 集成）、`cli.rs`（--config 参数）以及语义非法测试夹具。

整体架构清晰，三步链（读文件 → 反序列化 → 语义验证）职责分明，错误消息中文友好。发现 4 条 WARNING 和 3 条 INFO，无 BLOCKER 级别问题。最值得关注的是：Windows 平台默认路径硬编码为 Linux 路径（CLAUDE.md 明确要求支持 Windows）、端口合法性验证不完整、集成测试使用相对路径导致在特定工作目录下失败。

---

## Warnings

### WR-01: 默认安装路径在 Windows 上必然失败

**File:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/config/mod.rs:43-44`

**Issue:** `default_install_path()` 和 `default_data_path()` 硬编码返回 `/opt/dmdbms` 和 `/opt/dmdbms/data`。CLAUDE.md 的约束明确列出 "Platforms: Linux (x86/ARM) + Windows"，Phase 2 的 Rust 二进制正是面向 Windows 的路径。用户在 Windows 上不提供 `--config` 时，这两个路径将被写入 XML 响应文件并传给 `DMInstall.bin`，安装器会因路径非法而静默失败，且错误信息来自 DM 原生安装程序，完全没有用户友好提示。

**Fix:**
```rust
fn default_install_path() -> String {
    if cfg!(target_os = "windows") {
        r"C:\dmdbms".to_string()
    } else {
        "/opt/dmdbms".to_string()
    }
}

fn default_data_path() -> String {
    if cfg!(target_os = "windows") {
        r"C:\dmdbms\data".to_string()
    } else {
        "/opt/dmdbms/data".to_string()
    }
}
```

---

### WR-02: 端口验证只拒绝 0，特权端口 1-1023 静默通过

**File:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/config/mod.rs:89-91`

**Issue:** 当前逻辑仅检查 `port == 0`。注释和错误消息声称有效范围是 "1-65535"，但用户配置 `port = 80` 这类特权端口时验证通过。在 Linux 上以非 root 用户运行（或 systemd 服务未配置 `CAP_NET_BIND_SERVICE`）时，`dm` 服务启动会因 `EACCES` 失败。由于安装步骤在验证之后很久才执行，错误发生在 `step_dminit` 阶段，与配置验证阶段脱节，极难调试。

**Fix:**
```rust
if cfg.port == 0 {
    bail!("配置验证失败: port 无效: 0；有效范围为 1-65535");
}
if cfg.port < 1024 {
    bail!(
        "配置验证失败: port {} 为特权端口（< 1024），\
         需要 root 权限或 CAP_NET_BIND_SERVICE；建议使用 5236",
        cfg.port
    );
}
```

---

### WR-03: `test_semantic_invalid_fixture_rejected` 使用相对路径，工作目录不固定时失败

**File:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/config/validate.rs:81-82`

**Issue:**
```rust
config: "tests/fixtures/semantic_invalid.toml".into(),
```
`cargo test` 在 crate 根目录执行时此路径有效，但在工作区根目录执行（`cargo test -p dm-installer`）或 IDE 运行时，CWD 可能不同，测试会以 "无法读取配置文件" 失败，而不是预期的 "page_size 无效: 12"，产生误导性的错误信息。这是一个脆弱的测试，在 CI 环境（尤其是 `cross` 交叉编译测试）中存在失败风险。

**Fix:**
```rust
fn test_semantic_invalid_fixture_resolved() -> std::path::PathBuf {
    // 相对于本源文件计算绝对路径，不受 CWD 影响
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("tests/fixtures/semantic_invalid.toml")
}

#[test]
fn test_semantic_invalid_fixture_rejected() {
    let args = ValidateArgs { config: test_semantic_invalid_fixture_resolved() };
    let err = run(&args).unwrap_err();
    // ... 断言不变
}
```

---

### WR-04: `--config` 与 `--defaults` 正交但交互确认行为不一致，DBA 场景需要二次确认

**File:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/mod.rs:79`

**Issue:** `step_confirm_params` 的静默条件是 `args.defaults || args.yes`。DBA 提供 `--config /etc/dm.toml` 时（典型生产场景），若未同时传 `--defaults`，仍会触发交互确认提示。DBA 已通过配置文件明确表达了所有参数意图，再次弹出"确认参数？"提示既违反最小意外原则，也导致非 TTY 的管道脚本（CI/Ansible 调用）卡住等待输入。`cli.rs:153-160` 的测试也将 `--config` 与 `--defaults` 视为"正交可同时指定"，但这只是绕过症状而非修复根本。

**Fix:** 在 `step_confirm_params` 中追加 `args.config.is_some()` 条件：
```rust
fn step_confirm_params(args: &InstallArgs, config: &InstallConfig) -> Result<()> {
    tracing::info!("[5/7] 参数确认");
    // 提供了配置文件时视为已确认，无需交互
    let auto_confirm = args.defaults || args.yes || args.config.is_some();
    crate::ui::confirm_immutable_params(config, auto_confirm)
}
```

---

## Info

### IN-01: `load_and_validate` 无文件大小限制，超大文件将全量读入内存

**File:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/config/mod.rs:70-71`

**Issue:** `std::fs::read_to_string` 会将文件完整加载到内存。此程序以 root 身份运行，`--config` 指向的路径由用户指定。若路径指向一个大文件（如日志文件、块设备），可能造成大量内存分配后才报 TOML 解析错误。对于配置文件场景，合理的上限是 1MB。

**Fix:**
```rust
const MAX_CONFIG_SIZE: u64 = 1024 * 1024; // 1 MB
pub fn load_and_validate(path: &Path) -> Result<InstallConfig> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("无法读取配置文件: {}", path.display()))?;
    if metadata.len() > MAX_CONFIG_SIZE {
        bail!("配置文件过大 ({} 字节)，上限 1MB", metadata.len());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取配置文件: {}", path.display()))?;
    // ...
}
```

---

### IN-02: `fetch_package` 声明为 `async` 但 `Some` 分支无任何异步操作

**File:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/mod.rs:54-59`

**Issue:** 当 `args.package` 为 `Some` 时，函数立即返回 `Ok(p.clone())`，整个 async 框架在此路径上是零收益的开销，并且给读者造成"这里有 IO 操作"的误解。只有 `None` 分支实际触发 await。

**Fix:** 可保留 async 签名（为了统一调用方式），但添加注释说明：
```rust
async fn fetch_package(args: &InstallArgs) -> Result<std::path::PathBuf> {
    tracing::info!("[2/7] 获取安装包路径");
    match &args.package {
        // 本地路径直接返回，无 IO
        Some(p) => Ok(p.clone()),
        // 无本地包则触发网络下载（异步）
        None => crate::download::fetch_dm_installer().await,
    }
}
```

---

### IN-03: `--yes` 与 `--defaults` 语义重复，缺少 `conflicts_with` 约束

**File:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/cli.rs:46-47`

**Issue:** 文档注释说 `--yes` "等同于 --defaults"，两者在 `install/mod.rs:79` 中以 `args.defaults || args.yes` 合并判断，行为完全相同。同时存在两个相同语义的参数增加了 API 表面积，且没有 `conflicts_with` 互斥声明，用户同时传入两者时不会收到任何提示。若未来其中一个语义发生分化，修改成本较高。

**Fix:** 若必须同时保留（向后兼容或用户习惯），至少添加 `conflicts_with`：
```rust
/// 跳过确认，等同于 --defaults
#[arg(long, short = 'y', conflicts_with = "defaults")]
pub yes: bool,
```
或者将其中一个标记为已弃用：
```rust
#[arg(long, hide = true)]  // 保留但隐藏，推荐使用 --defaults
pub yes: bool,
```

---

_Reviewed: 2026-06-12T10:19:31Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
