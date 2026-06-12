# Phase 2: TOML 配置驱动单机 - Research

**Researched:** 2026-06-12
**Domain:** Rust CLI，serde/TOML 反序列化，clap derive，语义验证
**Confidence:** HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** `InstallArgs` 新增 `#[arg(long)] pub config: Option<PathBuf>`
- **D-02:** `install/mod.rs::run()` 内条件分支：`config` 存在时调 `config::load_and_validate(path)`，否则用 `InstallConfig::default()`
- **D-03:** `--config` 和 `--defaults` 正交：`--config` 决定参数来源，`--defaults` 控制是否跳过确认 UI
- **D-04:** `validate_install_config(cfg: &InstallConfig) -> Result<()>` 语义验证规则：
  - `page_size` ∈ {4, 8, 16, 32}
  - `charset` ∈ {0, 1, 2}
  - `extent_size` ∈ {16, 32}
  - `port` ∈ [1, 65535]（u16 类型已约束）
- **D-05:** 路径字段不做存在性校验
- **D-06:** `config::load_and_validate(path: &Path) -> Result<InstallConfig>`：读文件 → `toml::from_str` → `validate_install_config`
- **D-07:** Phase 1 `config/validate.rs::run()` 重构为调用 `load_and_validate()`
- **D-08:** `--config` 下 `ui::confirm_immutable_params()` 仍展示配置值并等待 y/n
- **D-09:** 只有 `--yes` 或 `--defaults` 才跳过确认，`--config` 本身不自动跳过

### Claude's Discretion

- TOML 字段名沿用 `InstallConfig` snake_case，无需 `#[serde(rename)]`
- 语义验证错误用 `anyhow::bail!()`，不新增 `thiserror` 类型
- `validate_install_config()` 是纯函数，不做 I/O

### Deferred Ideas (OUT OF SCOPE)

- `[cluster]` TOML 段落 — Phase 3
- `--dry-run` 模式 — v2 需求
- 断点续传 — v2 需求
- 自动下载 URL (DOWN-01 完整实现) — spike 待验证
</user_constraints>

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| INST-02 | 用户可通过 TOML 配置文件安装单机达梦，支持自定义端口、数据路径、页大小、字符集、大小写敏感等所有 dminit 参数 | `toml::from_str::<InstallConfig>()` 直接反序列化到已有结构体；`serde(default)` 处理缺省字段 |
| QUAL-03 | 用户可运行 `dm-installer validate --config config.toml` 仅验证配置文件合法性，不执行实际安装 | Phase 1 已有 `validate` 子命令骨架；Phase 2 提取 `load_and_validate()` 完成语义验证层 |
</phase_requirements>

---

## Summary

Phase 2 是在 Phase 1 已有 Rust 骨架上的增量修改，**不新建任何模块**。所有改动集中在三个文件：`src/cli.rs`（增加 `--config` 字段）、`src/config/mod.rs`（添加 `load_and_validate()`）、`src/install/mod.rs`（条件分支读取配置）。`src/config/validate.rs` 也需小幅重构以调用共用函数。

Phase 1 的代码设计已预留 Phase 2 扩展点：`InstallConfig` 有完整的 `serde::Deserialize` + `Default` impl，`confirm_immutable_params()` 接受 `&InstallConfig` 参数（不硬编码值），安装编排器 `install::run()` 在最前面构建 config 对象，天然适合插入条件分支。

最大风险点是语义验证的错误信息格式——这直接影响 INST-02 SC3（"配置文件格式错误时给出清晰错误信息"）。错误链需要正确传播到顶层并以中文展示。`anyhow::bail!` + `.with_context()` 的组合已在 Phase 1 代码中得到验证。

**Primary recommendation:** 严格按 D-06 链式调用（读文件 → TOML 解析 → 语义验证），利用 `?` 操作符自然传播错误，保持每层错误上下文清晰。

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| TOML 文件读取与反序列化 | `config::mod` | — | 配置模块职责边界 |
| 语义验证（枚举值域检查） | `config::mod` | — | 与 TOML 解析在同一模块，便于复用 |
| CLI flag 解析（--config） | `cli::InstallArgs` | — | clap derive 层，只做参数类型解析 |
| 安装流程编排 | `install::mod::run()` | — | 编排器负责决定用哪个 config 来源 |
| 不可修改参数确认 UI | `ui::confirm_immutable_params` | — | UI 层，接受 config 引用，不感知来源 |
| validate 子命令 | `config::validate::run()` | — | 调用 `load_and_validate()`，打印结果 |

---

## Standard Stack

### Core（Phase 2 使用的依赖子集）

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `clap` | 4.6.1 | `#[arg(long)] config: Option<PathBuf>` | 已在 Cargo.toml，derive macro 原生支持 `Option<PathBuf>` [VERIFIED: cargo search] |
| `serde` | 1.0.228 | `InstallConfig` 反序列化 | 已在 Cargo.toml，`#[serde(default = "fn")]` 处理缺省字段 [VERIFIED: cargo search] |
| `toml` | 1.1.2 | `toml::from_str::<T>()` | 已在 Cargo.toml，与 serde 直接兼容 [VERIFIED: cargo search] |
| `anyhow` | 1.0.102 | 验证错误传播 | 已在 Cargo.toml，`bail!()` + `with_context()` 模式已验证 [VERIFIED: cargo search] |

**无需新增依赖。** Phase 2 所需功能全部由 Phase 1 已有的 Cargo.toml 依赖覆盖。

### 版本验证

```
clap = "4.6.1"      (cargo search: 4.6.1 — 当前最新)
toml = "1.1.2"      (cargo search: 1.1.2+spec-1.1.0 — 当前最新)
anyhow = "1.0.102"  (cargo search: 1.0.102 — 当前最新)
```

[VERIFIED: cargo search 输出，2026-06-12]

---

## Package Legitimacy Audit

Phase 2 不引入任何新外部依赖，全部使用 Phase 1 已锁定的 Cargo.lock 版本。

**无需运行 slopcheck。**

---

## Architecture Patterns

### System Architecture Diagram

```
dm-installer install --config dm.toml --package /path/to/dm.iso
       │
       ▼
cli::InstallArgs { config: Some(PathBuf), package: Some(..), defaults: bool, yes: bool }
       │
       ▼
install::run(args)
       │
       ├─[args.config = Some(path)]──► config::load_and_validate(path)
       │                                     │
       │                              std::fs::read_to_string
       │                                     │
       │                              toml::from_str::<InstallConfig>
       │                                     │
       │                              validate_install_config(&cfg)
       │                                     │ page_size/charset/extent_size 枚举检查
       │                                     ▼
       │                              Ok(InstallConfig) / Err(anyhow)
       │
       ├─[args.config = None]─────► InstallConfig::default()
       │
       ▼
InstallConfig (配置来源透明，后续步骤不感知)
       │
       ├─► idempotent::check_existing_instance(&config)
       ├─► fetch_package(args)
       ├─► verify_checksum(args, &iso_path)
       ├─► package::extract_dminstall_bin(&iso_path)
       ├─► ui::confirm_immutable_params(&config, skip=args.defaults||args.yes)
       ├─► silent_install::run(&config, &extract_dir)
       └─► init::run_dminit(&config)

dm-installer validate --config dm.toml
       │
       ▼
config::validate::run(args)
       │
       ▼
config::load_and_validate(&args.config)
       │
       ▼
Ok → println!("配置文件合法: {path}") / Err → anyhow 错误链传至顶层
```

### Recommended Project Structure

```
src/
├── cli.rs              # InstallArgs 新增 config: Option<PathBuf>
├── config/
│   ├── mod.rs          # InstallConfig + load_and_validate() + validate_install_config()
│   └── validate.rs     # validate 子命令：重构调用 load_and_validate()
├── install/
│   └── mod.rs          # run() 增加条件分支，其余模块不改
└── (其余模块不改动)
```

### Pattern 1: load_and_validate 三步链

**What:** 读文件 → TOML 解析 → 语义验证，三步合一，调用点只看到一个函数
**When to use:** `install --config` 和 `validate --config` 两处

```rust
// Source: CONTEXT.md D-06 + Phase 1 validate.rs 骨架
pub fn load_and_validate(path: &Path) -> Result<InstallConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取配置文件: {}", path.display()))?;

    let cfg: InstallConfig = toml::from_str(&content)
        .with_context(|| "配置文件解析失败")?;

    validate_install_config(&cfg)?;
    Ok(cfg)
}
```

### Pattern 2: 语义验证纯函数

**What:** 对枚举字段做值域检查，错误信息直接指向具体字段和有效值
**When to use:** 在 `load_and_validate()` 内调用；也可独立用于单元测试

```rust
// Source: CONTEXT.md D-04
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
    Ok(())
}
```

### Pattern 3: install::run() 条件分支

**What:** 根据 `args.config` 决定配置来源，后续步骤不感知来源
**When to use:** `install::run()` 函数入口处

```rust
// Source: CONTEXT.md D-02
pub async fn run(args: &InstallArgs) -> Result<()> {
    tracing::info!("开始安装达梦数据库");

    let config = match &args.config {
        Some(path) => config::load_and_validate(path)?,
        None => InstallConfig::default(),
    };

    // 后续 7 步完全不变
    if check_idempotent_early_exit(&config)? {
        return Ok(());
    }
    // ...
}
```

### Pattern 4: validate 子命令重构

**What:** 去除重复解析代码，改为调用共用函数
**When to use:** `config/validate.rs::run()`

```rust
// Source: CONTEXT.md D-07
pub fn run(args: &ValidateArgs) -> Result<()> {
    config::load_and_validate(&args.config)?;
    println!("配置文件合法: {}", args.config.display());
    Ok(())
}
```

### Anti-Patterns to Avoid

- **在 validate 和 install 中分别写 TOML 解析逻辑：** 重复代码，验证规则不一致风险。用 `load_and_validate()` 统一入口。
- **`--config` 自动跳过确认 UI：** 用户写完配置文件仍需"最后确认"不可修改参数（INST-03 要求）。`--config` 和 `--yes/--defaults` 正交（D-03、D-09）。
- **对路径字段做存在性检查：** 安装器负责创建目录，不应在配置验证阶段报错（D-05）。
- **新增 thiserror 错误类型处理验证逻辑：** 验证是 application-level 逻辑，用 `anyhow::bail!()` 直接格式化中文错误信息（Claude's Discretion）。

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| TOML 解析 + serde 默认值 | 手动解析 key=value | `toml::from_str::<InstallConfig>()` | serde `#[default]` 自动处理缺省字段，边界情况处理完整 |
| 枚举合法值检查 | 复杂 match 分支 | `.contains()` on `[4u8, 8, 16, 32]` 数组 | 一行代码，值域即文档，测试覆盖直接穷举 |
| CLI Option<PathBuf> | 手动 String 转 Path | `#[arg(long)] config: Option<PathBuf>` | clap 自动处理 None/Some，路径类型安全 |

**Key insight:** Phase 2 的核心价值在"配置来源切换"和"语义验证"，两者都是轻量逻辑——利用已有库特性而不是手写解析是正确方向。

---

## Common Pitfalls

### Pitfall 1: serde 错误信息不传播到用户可读格式

**What goes wrong:** `toml::from_str` 返回的错误是 `toml::de::Error`，直接用 `?` 传播时，顶层 anyhow 打印的错误不包含字段名或行号上下文
**Why it happens:** `toml::de::Error` 包含行列信息，但 `.with_context()` 包装后原始信息仍在错误链里
**How to avoid:** 用 `.with_context(|| "配置文件解析失败")` 包装一层中文上下文，anyhow `{:#}` 格式会同时打印上下文和原始 TOML 错误（含行列）
**Warning signs:** 用 `{:?}` 打印错误而非 `{:#}`；测试 `test_invalid_toml_fails` 已覆盖此情况

### Pitfall 2: --config 与 --yes/--defaults 耦合

**What goes wrong:** 实现时误以为"提供了配置文件就不需要手动确认"，在 `run()` 里对 `args.config.is_some()` 判断跳过确认
**Why it happens:** 开发者直觉：配置文件已明确了参数，为什么还要确认？
**How to avoid:** INST-03 要求在 dminit 前明确展示不可修改参数，无论参数来源。只有 `--yes` 或 `--defaults` 才跳过确认（D-08、D-09）
**Warning signs:** `confirm_immutable_params` 调用处的 `skip` 参数从 `args.defaults || args.yes` 改成了包含 `args.config.is_some()`

### Pitfall 3: validate_install_config 在 load_and_validate 之外被单独调用

**What goes wrong:** 某处直接用 `InstallConfig::default()` 构造配置对象后调用 `validate_install_config`，此时默认值（page_size=8, charset=0, extent_size=16）全部合法，验证无意义
**Why it happens:** 函数是 pub 的，容易误用
**How to avoid:** 函数保持 pub 用于测试，但文档注释说明"此函数用于 TOML 反序列化后验证用户输入，不应对 Default 构造的 config 调用"
**Warning signs:** 测试中只测试 Default config 的验证通过，没有测试边界无效值

### Pitfall 4: port 字段的 u16 约束与 TOML 类型匹配

**What goes wrong:** TOML 中 `port = 0` 或 `port = 70000` 的处理
**Why it happens:** u16 上限 65535，toml crate 在解析 `port = 70000` 时会在反序列化阶段就报类型错误（超出 u16 范围），不会进入语义验证
**How to avoid:** D-04 已明确 port 不需额外检查（u16 类型已约束）；但 `port = 0` 在 u16 范围内合法，若要排除需在 `validate_install_config` 显式检查。CONTEXT.md D-04 的规则是 `port ∈ [1, 65535]`，即需要检查 `cfg.port == 0`
**Warning signs:** 测试中没有 port=0 的 reject 用例

### Pitfall 5: toml::from_str 对多余字段的处理

**What goes wrong:** 用户 TOML 文件含有 Phase 3 的 `[cluster]` 段落，Phase 2 解析时是否报错？
**Why it happens:** serde 默认行为是忽略未知字段（对 struct 而言），但 `toml` crate 的行为需确认
**How to avoid:** `toml::from_str` + serde Deserialize 对 struct 的默认行为是**忽略未知字段**——用户在 Phase 2 配置文件里写 `[cluster]` 段落不会导致解析失败，Phase 3 直接扩展 struct 即可 [ASSUMED — 基于训练知识，toml crate 遵循 serde 约定]
**Warning signs:** 如果未来添加 `#[serde(deny_unknown_fields)]` 则会破坏此兼容性

---

## Code Examples

### 完整 TOML 配置文件示例（来自 CONTEXT.md Specifics）

```toml
# Source: CONTEXT.md § Specific Ideas
install_path = "/opt/dmdbms"
data_path = "/opt/dmdbms/data"
instance_name = "DMSERVER"
port = 5237
page_size = 16
charset = 1
case_sensitive = true
extent_size = 32
```

### InstallArgs 新增 --config 字段

```rust
// Source: CONTEXT.md D-01 + cli.rs 现有模式
#[derive(clap::Args)]
pub struct InstallArgs {
    #[arg(long)]
    pub package: Option<PathBuf>,

    #[arg(long)]
    pub checksum: Option<String>,

    #[arg(long)]
    pub defaults: bool,

    #[arg(long, short = 'y')]
    pub yes: bool,

    // Phase 2 新增
    /// TOML 配置文件路径（可选；未提供时使用内置默认参数）
    #[arg(long)]
    pub config: Option<PathBuf>,
}
```

### 语义验证边界测试覆盖

```rust
// 测试思路：validate_install_config 是纯函数，直接穷举边界值
#[test]
fn test_invalid_page_size_rejected() {
    let cfg = InstallConfig { page_size: 12, ..InstallConfig::default() };
    let err = validate_install_config(&cfg).unwrap_err();
    assert!(format!("{}", err).contains("page_size 无效: 12"));
}

#[test]
fn test_valid_page_sizes_accepted() {
    for ps in [4u8, 8, 16, 32] {
        let cfg = InstallConfig { page_size: ps, ..InstallConfig::default() };
        assert!(validate_install_config(&cfg).is_ok(), "page_size={} 应合法", ps);
    }
}

#[test]
fn test_port_zero_rejected() {
    let cfg = InstallConfig { port: 0, ..InstallConfig::default() };
    let err = validate_install_config(&cfg).unwrap_err();
    assert!(format!("{}", err).contains("port"));
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Phase 1 validate.rs 独立解析 TOML | Phase 2 提取 `load_and_validate()` 共用 | Phase 2 | 消除重复解析代码，保证 install 和 validate 使用同一验证逻辑 |
| `install::run()` 硬编码 `InstallConfig::default()` | 条件分支：`--config` 路径 vs Default | Phase 2 | 支持 TOML 配置文件驱动安装（INST-02） |

**Deprecated/outdated after Phase 2:**
- `config/validate.rs` 中的内联 `toml::from_str` 调用：重构为调用 `config::load_and_validate()`

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `toml::from_str` + serde 对 struct 未知字段默认忽略（不报错） | Common Pitfalls #5 | 如果 toml crate 严格模式拒绝未知字段，Phase 2 用户写了 `[cluster]` 段落会报解析错误，需加 `#[allow_unknown_fields]` |

---

## Open Questions

1. **port = 0 是否需要拒绝？**
   - What we know: D-04 规则是 `port ∈ [1, 65535]`，即 port=0 应被拒绝
   - What's unclear: CONTEXT.md 写"u16 类型本身已约束，无需额外检查"——这与 `[1, 65535]` 的下界矛盾（u16 允许 0）
   - Recommendation: 在 `validate_install_config()` 增加 `if cfg.port == 0 { bail!(...) }` 检查，与 D-04 的文字描述保持一致

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust / cargo | 编译 dm-installer | ✓ | rustc 1.96.0 / cargo 1.96.0 | — |
| cargo-nextest | 快速并行测试 | ✗ | — | `cargo test` |

**Missing dependencies with no fallback:** 无

**Missing dependencies with fallback:**
- `cargo-nextest`: 不影响 Phase 2 执行，测试命令改用 `cargo test` 即可

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) |
| Config file | Cargo.toml `[dev-dependencies]` |
| Quick run command | `cargo test` |
| Full suite command | `cargo test -- --include-ignored` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| INST-02 | TOML 文件所有字段均生效（port/page_size/charset 等） | unit | `cargo test config::` | ❌ Wave 0 — 需新增 `validate_install_config` 测试 |
| INST-02 SC1 | TOML 文件驱动完整安装（端口/路径/页大小均按 config 执行） | integration | `cargo test install::` (mock DMInstall.bin) | ❌ Wave 0 |
| INST-02 SC2 | 所有 dminit 参数均在 XML 响应文件中体现 | unit | `cargo test install::silent_install::` | ✅ 已有 `test_xml_contains_all_required_tags` |
| INST-02 SC3 | 配置错误时给出字段名清晰错误信息 | unit | `cargo test config::` | ❌ Wave 0 — 需新增验证错误消息断言测试 |
| INST-02 SC4 | install_path/data_path/port 等均使用 config 文件值 | unit | `cargo test install::` | ❌ Wave 0 — 需新增 config 注入测试 |
| QUAL-03 | `validate --config` 不执行安装，仅验证 | unit | `cargo test config::validate::` | ✅ Phase 1 已有三个 validate 测试（语法验证），需补语义验证用例 |

### Sampling Rate

- **Per task commit:** `cargo test`
- **Per wave merge:** `cargo test`
- **Phase gate:** 全套绿 before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src/config/mod.rs` 中 `validate_install_config()` 的单元测试 — 覆盖 INST-02 SC3
  - 测试：page_size 无效值拒绝、charset 无效值拒绝、extent_size 无效值拒绝、所有有效值通过
  - 测试：port=0 拒绝（若 D-04 要求 ≥1）
- [ ] `src/config/mod.rs` 中 `load_and_validate()` 的集成测试（使用 tempfile fixture）— 覆盖 INST-02
- [ ] `src/install/mod.rs` 的 `--config` 条件分支测试 — 覆盖 INST-02 SC4
- [ ] `tests/fixtures/semantic_invalid.toml` — page_size=12 的语义非法 fixture（区别于现有语法错误 fixture）

---

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — |
| V3 Session Management | no | — |
| V4 Access Control | no | — |
| V5 Input Validation | yes | `validate_install_config()` 枚举值域检查 |
| V6 Cryptography | no | — |

### Known Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| TOML 路径字段 XML 注入 | Tampering | `xml_escape()` 已在 `silent_install.rs` 实现，`InstallConfig` 字段通过此函数转义 |
| 任意路径写入（install_path 指向系统目录） | Elevation of Privilege | 安装器需 root 权限运行（Phase 1 已有 sudo re-exec 逻辑）；路径合法性由 OS 权限检查决定 |

---

## Sources

### Primary (HIGH confidence)
- Phase 1 worktree 源码（`src/config/mod.rs`、`src/cli.rs`、`src/install/mod.rs`、`src/config/validate.rs`）— 直接读取，零假设
- `CONTEXT.md` D-01 至 D-09 — 用户锁定决策，权威来源
- `CLAUDE.md §Technology Stack` — 库版本（clap 4.6.1、serde 1.0.228、toml 1.1.2、anyhow 1.0.102）
- cargo search 输出（2026-06-12）— 验证 clap=4.6.1、toml=1.1.2、anyhow=1.0.102 均为当前最新版本

### Secondary (MEDIUM confidence)
- [serde.rs field-attrs](https://serde.rs/field-attrs.html) — `#[serde(default = "fn")]` 行为
- [docs.rs/clap derive tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) — `Option<PathBuf>` 与 `#[arg(long)]` 的组合用法

### Tertiary (LOW confidence)
- A1（toml 未知字段忽略行为）— 基于训练知识，未在此 session 通过 Context7 或官方文档确认

---

## Metadata

**Confidence breakdown:**
- Standard Stack: HIGH — cargo search 验证版本，Phase 1 Cargo.toml 已包含所有依赖
- Architecture: HIGH — 基于 Phase 1 实际代码，不依赖假设
- Pitfalls: HIGH（Pitfall 1-4）/ MEDIUM（Pitfall 5，A1 标记）
- Test Map: HIGH — 基于现有测试文件直接检查

**Research date:** 2026-06-12
**Valid until:** 2026-07-12（依赖稳定，变化风险低）
