# Phase 2: TOML 配置驱动单机 - Pattern Map

**Mapped:** 2026-06-12
**Files analyzed:** 5 (4 modified + 1 new fixture)
**Analogs found:** 5 / 5

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/cli.rs` | config | request-response | `src/cli.rs` itself (Phase 1 pattern) | exact — same file, add one field |
| `src/config/mod.rs` | service | transform | `src/config/validate.rs` (TOML parse chain) | exact — same parse + context pattern |
| `src/config/validate.rs` | service | request-response | `src/config/validate.rs` itself (Phase 1) | exact — same file, shrink to thin wrapper |
| `src/install/mod.rs` | controller | request-response | `src/install/mod.rs` itself (Phase 1) | exact — same file, add conditional branch |
| `tests/fixtures/semantic_invalid.toml` | config | — | `tests/fixtures/invalid.toml` | exact — same fixture format |

---

## Pattern Assignments

### `src/cli.rs` — 新增 `config: Option<PathBuf>` 字段

**Analog:** 同文件 Phase 1，`ValidateArgs` 已示范 `PathBuf` 字段用法

**现有 `InstallArgs` 结构（lines 31-48）：**
```rust
/// install 子命令参数
#[derive(clap::Args)]
pub struct InstallArgs {
    /// 本地 ISO 安装包路径
    #[arg(long)]
    pub package: Option<PathBuf>,

    /// 可选的 SHA-256 校验和（十六进制字符串）
    #[arg(long)]
    pub checksum: Option<String>,

    /// 跳过所有交互确认（curl | sh 模式使用）
    #[arg(long)]
    pub defaults: bool,

    /// 跳过确认，等同于 --defaults
    #[arg(long, short = 'y')]
    pub yes: bool,
}
```

**`Option<PathBuf>` 字段模式（同文件 ValidateArgs，lines 51-56）：**
```rust
#[derive(clap::Args)]
pub struct ValidateArgs {
    /// TOML 配置文件路径
    #[arg(long)]
    pub config: PathBuf,
}
```

**新增字段须仿照 `package` 字段：**
- `Option<PathBuf>` — clap 自动处理 None/Some
- `#[arg(long)]` — 与所有现有 long flags 一致
- 中文 doc comment — 与 `package`/`checksum`/`defaults`/`yes` 注释风格一致

**新增后的字段（追加到 InstallArgs 末尾）：**
```rust
    /// TOML 配置文件路径（可选；未提供时使用内置默认参数）
    #[arg(long)]
    pub config: Option<PathBuf>,
```

**测试模式（lines 58-121）：**
```rust
#[test]
fn test_install_args_defaults() {
    let cli = Cli::try_parse_from(["dm-installer", "install", "--defaults"]).unwrap();
    let Commands::Install(args) = cli.command else {
        panic!("expected Install command");
    };
    assert!(args.defaults, "--defaults 应解析为 true");
}
```
Phase 2 测试应复用此模式，覆盖 `--config /path/to/dm.toml` 解析为 `Some(PathBuf)` 的场景。

---

### `src/config/mod.rs` — 新增 `load_and_validate()` 和 `validate_install_config()`

**Analog:** `src/config/validate.rs`（Phase 1 的 TOML parse + context chain）

**现有 TOML 解析三步链模式（validate.rs lines 1-18）：**
```rust
use anyhow::{Context, Result};

pub fn run(args: &ValidateArgs) -> Result<()> {
    let content = std::fs::read_to_string(&args.config)
        .with_context(|| format!("无法读取配置文件: {}", args.config.display()))?;

    toml::from_str::<InstallConfig>(&content)
        .with_context(|| "配置文件解析失败")?;

    println!("配置文件合法: {}", args.config.display());
    Ok(())
}
```
`load_and_validate()` 将此链提取为独立函数，加第三步语义验证。

**现有 serde default 模式（mod.rs lines 1-63）：**
```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct InstallConfig {
    #[serde(default = "default_page_size")]
    pub page_size: u8,

    #[serde(default = "default_charset")]
    pub charset: u8,

    #[serde(default = "default_extent_size")]
    pub extent_size: u8,
    // ...
}

fn default_page_size() -> u8 { 8 }
fn default_charset() -> u8 { 0 }
fn default_extent_size() -> u8 { 16 }
```
`validate_install_config()` 直接访问这些字段，类型已是 `u8`，`.contains()` 方法用 `[4u8, 8, 16, 32]` 数组检查。

**错误处理模式（项目统一风格）：**
- `anyhow::bail!()` 格式化中文错误消息（无需 `thiserror`）
- `.with_context(|| "...")` 包装每层错误，anyhow `{:#}` 格式同时打印上下文和原始错误（含行列）

**`load_and_validate()` 完整实现模式：**
```rust
use std::path::Path;
use anyhow::{Context, Result};

pub fn load_and_validate(path: &Path) -> Result<InstallConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取配置文件: {}", path.display()))?;

    let cfg: InstallConfig = toml::from_str(&content)
        .with_context(|| "配置文件解析失败")?;

    validate_install_config(&cfg)?;
    Ok(cfg)
}
```

**`validate_install_config()` 完整实现模式：**
```rust
pub fn validate_install_config(cfg: &InstallConfig) -> Result<()> {
    if ![4u8, 8, 16, 32].contains(&cfg.page_size) {
        anyhow::bail!(
            "配置验证失败: page_size 无效: {}；有效值为 4/8/16/32",
            cfg.page_size
        );
    }
    if ![0u8, 1, 2].contains(&cfg.charset) {
        anyhow::bail!(
            "配置验证失败: charset 无效: {}；有效值 0=GB18030 1=UTF-8 2=EUC-KR",
            cfg.charset
        );
    }
    if ![16u8, 32].contains(&cfg.extent_size) {
        anyhow::bail!(
            "配置验证失败: extent_size 无效: {}；有效值为 16/32",
            cfg.extent_size
        );
    }
    if cfg.port == 0 {
        anyhow::bail!("配置验证失败: port 无效: 0；有效范围为 1-65535");
    }
    Ok(())
}
```

**测试模式（validate.rs lines 20-84）：**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use crate::config::InstallConfig;

    #[test]
    fn test_invalid_toml_fails() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"port = "not_a_number""#).unwrap();
        let args = ValidateArgs { config: file.path().to_path_buf() };
        let err = run(&args).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("配置文件解析失败"),
            "错误链应包含'配置文件解析失败'，实际: {msg}"
        );
    }
}
```
Phase 2 单元测试复用 `InstallConfig { page_size: 12, ..InstallConfig::default() }` 语法构造边界值，不依赖 tempfile。

---

### `src/config/validate.rs` — 重构为调用 `load_and_validate()`

**Analog:** 同文件 Phase 1（lines 1-18）

**现有实现（将被替换）：**
```rust
pub fn run(args: &ValidateArgs) -> Result<()> {
    let content = std::fs::read_to_string(&args.config)
        .with_context(|| format!("无法读取配置文件: {}", args.config.display()))?;

    toml::from_str::<InstallConfig>(&content)
        .with_context(|| "配置文件解析失败")?;

    println!("配置文件合法: {}", args.config.display());
    Ok(())
}
```

**重构后目标模式（Phase 2）：**
```rust
pub fn run(args: &ValidateArgs) -> Result<()> {
    super::load_and_validate(&args.config)?;
    println!("配置文件合法: {}", args.config.display());
    Ok(())
}
```

**保留的测试（lines 20-84）：**
- `test_valid_toml_passes` — 直接保留，行为不变
- `test_invalid_toml_fails` — 保留，断言 `"配置文件解析失败"` 仍成立（错误链不变）
- `test_missing_file_fails` — 保留，断言 `"无法读取配置文件"` 仍成立
- `test_install_config_defaults` — 保留，验证 Default 值不受 Phase 2 改动影响
- `test_install_config_partial_toml` — 保留，验证 serde default 行为

**新增语义验证测试须补充：**
```rust
#[test]
fn test_semantic_invalid_toml_fails() {
    // 使用 tests/fixtures/semantic_invalid.toml（page_size=12）
    let args = ValidateArgs {
        config: "tests/fixtures/semantic_invalid.toml".into()
    };
    let err = run(&args).unwrap_err();
    let msg = format!("{:#}", err);
    assert!(msg.contains("page_size 无效: 12"));
}
```

---

### `src/install/mod.rs` — 在 `run()` 入口处添加条件分支

**Analog:** 同文件 Phase 1（lines 17-35）

**现有 `run()` 入口（lines 17-35）：**
```rust
pub async fn run(args: &InstallArgs) -> Result<()> {
    tracing::info!("开始安装达梦数据库");
    let config = InstallConfig::default();

    if check_idempotent_early_exit(&config)? {
        return Ok(());
    }

    let iso_path = fetch_package(args).await?;
    verify_checksum(args, &iso_path)?;

    let extract_dir = step_extract(&iso_path)?;
    step_confirm_params(args, &config)?;
    step_silent_install(&config, &extract_dir)?;
    step_dminit(&config)?;

    crate::ui::print_status(StatusLevel::Info, "Plan 04 将注册 systemd 服务");
    Ok(())
}
```

**修改模式：仅替换第 3 行（`let config = ...`），其余 7 步完全不变：**
```rust
let config = match &args.config {
    Some(path) => crate::config::load_and_validate(path)?,
    None => InstallConfig::default(),
};
```

**`tracing::info!` 步骤日志模式（lines 37-82）：**
```rust
fn check_idempotent_early_exit(config: &InstallConfig) -> Result<bool> {
    tracing::info!("[1/7] 幂等性检测");
    // ...
}
```
格式 `"[N/7] 步骤名"` 在全文件一致，Phase 2 不改变此格式。

**`step_confirm_params` 模式（lines 69-72）：**
```rust
fn step_confirm_params(args: &InstallArgs, config: &InstallConfig) -> Result<()> {
    tracing::info!("[5/7] 参数确认");
    crate::ui::confirm_immutable_params(config, args.defaults || args.yes)
}
```
注意：`skip` 参数为 `args.defaults || args.yes`，不包含 `args.config.is_some()`（D-09）。Phase 2 **不修改此函数**。

---

### `tests/fixtures/semantic_invalid.toml` — 新建语义非法 fixture

**Analog:** `tests/fixtures/invalid.toml`（语法错误 fixture）

**现有语法错误 fixture（invalid.toml）：**
```toml
port = "not_a_number"
```

**现有有效 fixture（valid.toml）：**
```toml
port = 5237
page_size = 16
```

**新建语义非法 fixture 模式（语法合法但值域非法）：**
```toml
page_size = 12
```
含义：TOML 语法合法（整数），但 `page_size` 不在 {4,8,16,32} 中，触发 `validate_install_config()` 错误路径。此 fixture 区分"语法错误"和"语义错误"两类测试场景。

---

## Shared Patterns

### 错误处理链（适用所有修改文件）

**Source:** `src/config/validate.rs` lines 1-18
**Apply to:** `config/mod.rs` 中的两个新函数，`install/mod.rs` 中的条件分支

```rust
// 一致模式：每层 .with_context(|| "中文描述") + 末尾 ? 传播
let content = std::fs::read_to_string(path)
    .with_context(|| format!("无法读取配置文件: {}", path.display()))?;

// application-level 验证用 bail!，不新增 thiserror 类型
anyhow::bail!("配置验证失败: page_size 无效: {}；有效值为 4/8/16/32", cfg.page_size);
```

错误打印时用 `{:#}` 格式（anyhow "pretty" 格式），同时打印所有上下文层 + 原始错误。

### tracing 日志模式（适用 install/mod.rs）

**Source:** `src/install/mod.rs` lines 18, 38, 47, 54, 65, 70, 74, 80
**Apply to:** `install/mod.rs` 条件分支前后的 `tracing::info!` 调用

```rust
tracing::info!("开始安装达梦数据库");  // run() 第一行
tracing::info!("[1/7] 幂等性检测");   // 每个步骤函数第一行
```

Phase 2 在 `run()` 中新增的条件分支不需要额外 `tracing::info!` 调用——配置来源切换是内部逻辑，非用户可见步骤。

### serde default 函数模式（适用 config/mod.rs）

**Source:** `src/config/mod.rs` lines 41-48
**Apply to:** Phase 2 不新增字段，此模式不变，但 `validate_install_config()` 依赖这些函数返回值作为默认测试基线

```rust
fn default_page_size() -> u8 { 8 }   // 合法值，测试时用 ..InstallConfig::default() 作基线
fn default_charset() -> u8 { 0 }    // 合法值
fn default_extent_size() -> u8 { 16 } // 合法值
```

### 测试中的 clap 解析模式（适用 cli.rs）

**Source:** `src/cli.rs` lines 63-121
**Apply to:** Phase 2 新增的 `--config` 解析测试

```rust
let cli = Cli::try_parse_from(["dm-installer", "install", "--config", "/etc/dm.toml"]).unwrap();
let Commands::Install(args) = cli.command else {
    panic!("expected Install command");
};
assert_eq!(args.config, Some(PathBuf::from("/etc/dm.toml")));
```

---

## No Analog Found

无。所有 Phase 2 文件均有直接 analog（Phase 1 代码）。

---

## Metadata

**Analog search scope:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/`, `tests/fixtures/`
**Files scanned:** 6（cli.rs, config/mod.rs, config/validate.rs, install/mod.rs, tests/fixtures/valid.toml, tests/fixtures/invalid.toml）
**Pattern extraction date:** 2026-06-12
