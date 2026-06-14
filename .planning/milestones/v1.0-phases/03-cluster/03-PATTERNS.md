# Phase 3: 主备集群 - Pattern Map

**Mapped:** 2026-06-12
**Files analyzed:** 11 (新建/修改文件)
**Analogs found:** 10 / 11

---

## File Classification

| 新建/修改文件 | 角色 | 数据流 | 最近 Analog | 匹配质量 |
|---|---|---|---|---|
| `src/cli.rs` | config (修改) | request-response | `src/cli.rs` (自身) | exact — 新增 `Commands::Cluster` variant |
| `src/main.rs` | config (修改) | request-response | `src/main.rs` (自身) | exact — 新增 `Commands::Cluster` 分发分支 |
| `src/config/cluster.rs` | model | CRUD | `src/config/mod.rs` | role-match — serde Deserialize 结构体，同一 TOML 反序列化模式 |
| `src/cluster/mod.rs` | service (orchestrator) | event-driven | `src/install/mod.rs` | role-match — 顶层 `run()` 编排器，步骤链模式 |
| `src/cluster/ssh.rs` | service | request-response | `src/install/silent_install.rs` | partial — 执行外部命令 + 捕获输出，ssh 版本无 codebase 先例 |
| `src/cluster/preflight.rs` | service | request-response | `src/install/mod.rs` | partial — 多步检查链，`?` 传播，tokio async |
| `src/cluster/deploy.rs` | service | event-driven | `src/install/mod.rs` | role-match — 安装编排，步骤函数分拆 |
| `src/cluster/health.rs` | utility | request-response | `src/install/checksum.rs` | partial — 单一职责辅助函数，`anyhow::Result` 返回 |
| `src/cluster/templates/mod.rs` | utility | transform | `src/install/silent_install.rs` | role-match — `format!` 模板字符串生成，测试覆盖输出内容 |
| `src/cluster/templates/dm_ini.rs` | utility | transform | `src/install/silent_install.rs` | role-match — 同上 |
| `src/cluster/templates/dmmal_ini.rs` | utility | transform | `src/install/silent_install.rs` | role-match — 同上 |
| `src/cluster/templates/dmarch_ini.rs` | utility | transform | `src/install/silent_install.rs` | role-match — 同上 |
| `src/cluster/templates/dmwatcher_ini.rs` | utility | transform | `src/install/silent_install.rs` | role-match — 同上 |

---

## Pattern Assignments

### `src/cli.rs` (修改 — 新增 `Commands::Cluster` variant)

**Analog:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/cli.rs`

**当前 Commands 枚举** (lines 17-28):
```rust
#[derive(Subcommand)]
pub enum Commands {
    /// 安装达梦数据库单机实例
    Install(InstallArgs),
    /// 验证 TOML 配置文件合法性（不执行安装）
    Validate(ValidateArgs),
    /// 生成 shell 补全脚本
    Completions {
        /// 目标 shell 类型（bash/zsh/fish 等）
        shell: clap_complete::Shell,
    },
}
```

**新增 variant 模式** — 参照 `Validate(ValidateArgs)` 的写法，新增：
```rust
/// 集群部署子命令
Cluster(ClusterArgs),
```

**ClusterArgs 新建方式** — 参照 `ValidateArgs`（lines 55-60）的必填 `--config`：
```rust
/// validate 子命令参数
#[derive(clap::Args)]
pub struct ValidateArgs {
    /// TOML 配置文件路径
    #[arg(long)]
    pub config: PathBuf,
}
```

`ClusterArgs` 使用相同结构：`#[arg(long)]` 必填 `config: PathBuf`，下含嵌套子命令 `ClusterSubcommand::Deploy`。

**测试模式** (lines 96-114):
```rust
#[test]
fn test_validate_args_config() {
    let cli = Cli::try_parse_from(["dm-installer", "validate", "--config", "/etc/dm.toml"]).unwrap();
    let Commands::Validate(args) = cli.command else {
        panic!("expected Validate command");
    };
    assert_eq!(args.config, PathBuf::from("/etc/dm.toml"), "...");
}

#[test]
fn test_validate_requires_config() {
    let result = Cli::try_parse_from(["dm-installer", "validate"]);
    assert!(result.is_err(), "validate 不带 --config 应解析失败");
}
```
`cluster deploy` 需要相同的两个测试：带 `--config` 通过、不带 `--config` 报错。

---

### `src/main.rs` (修改 — 新增 dispatch 分支)

**Analog:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/main.rs`

**当前 match 模式** (lines 25-36):
```rust
match &cli_args.command {
    cli::Commands::Install(args) => install::run(args).await,
    cli::Commands::Validate(args) => config::validate::run(args),
    cli::Commands::Completions { shell } => {
        use clap::CommandFactory;
        use clap_complete::generate;
        let mut cmd = cli::Cli::command();
        generate(*shell, &mut cmd, "dm-installer", &mut std::io::stdout());
        Ok(())
    }
}
```

新增分支与 `Install` 完全对称：
```rust
cli::Commands::Cluster(args) => cluster::run(args).await,
```

同时在顶部 `mod` 列表新增 `mod cluster;`，与现有 `mod install;` 并列。

---

### `src/config/cluster.rs` (新建)

**Analog:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/config/mod.rs`

**结构体 + serde 默认值模式** (lines 1-65):
```rust
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// InstallConfig 的完整模式：
/// 1. #[derive(Debug, Deserialize)]
/// 2. 每个有默认值的字段用 #[serde(default = "fn_name")]
/// 3. 对应的私有 fn default_xxx() -> T
/// 4. impl Default for T { fn default() -> Self { Self { field: default_fn(), ... } } }
#[derive(Debug, Deserialize)]
pub struct InstallConfig {
    #[serde(default = "default_install_path")]
    pub install_path: String,
    // ... 每个字段独立默认值函数
}

fn default_install_path() -> String { "/opt/dmdbms".to_string() }
```

`ClusterConfig` / `NodeConfig` / `SshCredentials` 复用此完整模式：
- `ClusterConfig` 含 `installer_package: PathBuf` 和 `nodes: Vec<NodeConfig>`
- `NodeConfig` 含 `role / host / port / instance_name / install_path / data_path / mal_port / dw_port / inst_dw_port / ssh: SshCredentials`
- `SshCredentials` 含 `user: String / identity_file: Option<PathBuf> / password: Option<String>`

**`load_and_validate` 三步链模式** (lines 69-76):
```rust
pub fn load_and_validate(path: &Path) -> Result<InstallConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("无法读取配置文件: {}", path.display()))?;
    let cfg = toml::from_str::<InstallConfig>(&content)
        .with_context(|| "配置文件解析失败")?;
    validate_install_config(&cfg)?;
    Ok(cfg)
}
```

`load_cluster_config(path)` 复用同一三步链：`read_to_string` → `toml::from_str::<ClusterConfig>` → `validate_cluster_config`。

**validate 函数模式** (lines 79-93):
```rust
pub fn validate_install_config(cfg: &InstallConfig) -> Result<()> {
    if ![4u8, 8, 16, 32].contains(&cfg.page_size) {
        bail!("配置验证失败: page_size 无效: {}；有效值为 4/8/16/32", cfg.page_size);
    }
    if cfg.port == 0 {
        bail!("配置验证失败: port 无效: 0；有效范围为 1-65535");
    }
    Ok(())
}
```

`validate_cluster_config` 对应的检查：
- nodes 列表非空
- 恰好含一个 `role = "primary"` 节点
- 每个节点 `port != 0`，`mal_port != port`（端口不冲突）
- SSH 凭据至少提供 identity_file 或 password 之一（D-06）
- `oguid` 在合法范围 0–2147483647

**测试模式** (lines 95-190) — 每个 validate 失败场景一个 `#[test]`，`assert!(msg.contains("...期望文字..."))` 断言错误信息包含关键字。

---

### `src/cluster/mod.rs` (新建 — 集群编排入口)

**Analog:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/mod.rs`

**编排器模式** (lines 21-43):
```rust
/// 安装子命令入口（INST-01 完整编排器）。
///
/// 流程：幂等检测 → 包路径 → checksum → ISO 提取 → 参数确认 → DMInstall.bin → dminit
pub async fn run(args: &InstallArgs) -> Result<()> {
    tracing::info!("开始安装达梦数据库");
    let config = resolve_config(args)?;

    if check_idempotent_early_exit(&config)? {
        return Ok(());
    }

    let iso_path = fetch_package(args).await?;
    verify_checksum(args, &iso_path)?;
    // ...每个步骤独立函数，< 40 行
    Ok(())
}
```

`cluster::run(args)` 复用此模式，步骤序列为：
```
load_cluster_config → preflight_all_nodes → concurrent_install → distribute_configs → ordered_startup → start_watchers
```

**步骤函数拆分模式** (lines 45-90) — 每个步骤 `fn step_xxx(...)` 独立，内部 `tracing::info!("[N/M] 步骤名")` 记录进度，函数本身 < 40 行。

---

### `src/cluster/ssh.rs` (新建 — russh 封装)

**无直接代码库 analog**（项目内无现有 SSH 代码）— 使用 RESEARCH.md Pattern 1 和 Pattern 2。

**imports 模式** — 与项目其他文件的 `use anyhow::{Context, Result};` 一致，新增：
```rust
use russh::{client, ChannelMsg};
use russh_sftp::client::SftpSession;
use thiserror::Error;
```

**thiserror 错误类型模式** — 参照 RESEARCH.md 的 Claude's Discretion：
```rust
#[derive(Debug, Error)]
pub enum SshError {
    #[error("SSH 连接失败 {host}: {source}")]
    Connect { host: String, #[source] source: russh::Error },
    #[error("SSH 命令执行失败 (exit {exit_code}): {command}")]
    ExecFailed { command: String, exit_code: u32 },
    #[error("SFTP 上传失败 {remote_path}: {source}")]
    SftpUpload { remote_path: String, #[source] source: russh_sftp::Error },
}
```

此模式与项目约定一致：模块内 `thiserror` 类型化错误，边界处 `anyhow` 包装（`SshError` 实现 `std::error::Error`，`anyhow::Context` 可直接包装）。

**exec_remote 函数模式** — 参照 RESEARCH.md Pattern 1（完整 ChannelMsg loop，同时匹配 Eof 和 None 两个 break 条件，防止 Pitfall 6 死循环）。

**sftp_write 函数模式** — 参照 RESEARCH.md Pattern 2（`channel_open_session` → `request_subsystem("sftp")` → `SftpSession::new(channel.into_stream())`）。

---

### `src/cluster/preflight.rs` (新建 — SSH 预检查)

**Analog:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/mod.rs` (编排模式 + 步骤函数)

**步骤函数拆分** (install/mod.rs lines 45-90) — 每项检查独立 async fn：
```rust
async fn check_sudo_nopass(ssh: &mut SshSession) -> Result<()> { ... }
async fn check_port_available(ssh: &mut SshSession, port: u16) -> Result<()> { ... }
async fn check_disk_space(ssh: &mut SshSession, parent_path: &str) -> Result<()> { ... }
```

**tracing 进度日志** — 对应 install/mod.rs 各步骤的 `tracing::info!("[N/7] ...")` 模式，preflight 使用节点前缀：
```rust
tracing::info!("[预检查] {} ({})", node.host, node.role);
```

**并发执行模式** (RESEARCH.md Pattern 3):
```rust
use futures::future::join_all;
let checks = nodes.iter().map(|node| check_node(node, ...));
let results: Vec<_> = join_all(checks).await;
let failures: Vec<_> = results.iter().enumerate()
    .filter(|(_, r)| r.is_err())
    .collect();
if !failures.is_empty() {
    anyhow::bail!("预检查失败 — 中止部署");
}
```

---

### `src/cluster/deploy.rs` (新建 — 节点安装编排)

**Analog:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/mod.rs` + `src/install/init.rs`

**步骤函数拆分模式** (install/mod.rs lines 45-90)

**`build_dminit_command` 参数模式** (init.rs lines 30-46):
```rust
/// 关键约束：dminit 参数等号两侧不能有空格。
/// 每个参数用 `.arg(format!("KEY={}", value))` 单独传递。
pub(crate) fn build_dminit_command(config: &InstallConfig) -> Vec<String> {
    vec![
        format!("{}/bin/dminit", config.install_path),
        format!("PATH={}", config.data_path),
        format!("INSTANCE_NAME={}", config.instance_name),
        format!("PORT_NUM={}", config.port),
        // ...
    ]
}
```

集群版 `build_dminit_args(node: &NodeConfig) -> Vec<String>` 复用此模式，注意 `INSTANCE_NAME` 主备不同（Pitfall 2 防范）。

**anyhow::ensure! 模式** (init.rs lines 19-24):
```rust
anyhow::ensure!(
    status.success(),
    "dminit 返回非零退出码: {:?}",
    status.code()
);
```

SSH exec 版本用 `SshError::ExecFailed` 代替 `status.success()` 检查。

---

### `src/cluster/health.rs` (新建 — TCP 健康轮询)

**Analog:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/checksum.rs`

**单一职责辅助函数模式** (checksum.rs lines 1-38):
```rust
use anyhow::Result;
// 只做一件事，pub fn 入口 + private impl 函数
pub fn verify_sha256(path: &Path, expected_hex: &str) -> Result<()> {
    let actual = compute_sha256(path)?;
    // ...
}
fn compute_sha256(path: &Path) -> Result<String> { ... }
```

`health.rs` 的 `wait_tcp_ready` 遵循同一结构：`pub async fn wait_tcp_ready(...)` + 内部 loop 拆为 private helper。

**TCP 轮询模式** (RESEARCH.md Pattern 4):
```rust
pub async fn wait_tcp_ready(host: &str, port: u16, max_secs: u64) -> anyhow::Result<()> {
    let addr = format!("{}:{}", host, port);
    let interval = tokio::time::Duration::from_secs(3);
    let deadline = tokio::time::Duration::from_secs(max_secs);
    tokio::time::timeout(deadline, async {
        loop {
            match tokio::net::TcpStream::connect(&addr).await {
                Ok(_) => return Ok(()),
                Err(_) => tokio::time::sleep(interval).await,
            }
        }
    }).await
    .map_err(|_| anyhow::anyhow!("主节点 {}:{} 在 {}s 内未就绪", host, port, max_secs))?
}
```

**测试模式** (checksum.rs lines 43-87) — 每个路径一个 `#[test]`，`assert!(err_msg.contains("...关键字..."))` 验证错误消息。

---

### `src/cluster/templates/mod.rs` + 各 `*_ini.rs` (新建)

**Analog:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/silent_install.rs`

**`format!` 模板生成模式** (silent_install.rs lines 17-53):
```rust
pub(crate) fn generate_install_xml(config: &InstallConfig) -> Result<NamedTempFile> {
    let install_path = xml_escape(&config.install_path);
    let xml = format!(
        r#"<?xml version="1.0"?>
<DATABASE>
  <INSTALL_PATH>{install_path}</INSTALL_PATH>
  <PORT_NUM>{port}</PORT_NUM>
</DATABASE>"#,
        port = config.port,
    );
    let mut file = NamedTempFile::new().context("创建 XML 临时文件失败")?;
    file.write_all(xml.as_bytes())?;
    Ok(file)
}
```

各 INI 模板函数遵循同一模式（但返回 `String` 而非写入文件，控制机生成后再 SFTP 推送，D-11）：
```rust
pub fn generate_dmmal_ini(nodes: &[NodeConfig]) -> String {
    let mut out = String::from("MAL_CHECK_INTERVAL = 5\nMAL_CONN_FAIL_INTERVAL = 5\n\n");
    for (i, node) in nodes.iter().enumerate() {
        out.push_str(&format!(
            "[MAL_INST{}]\nMAL_INST_NAME = {}\n...\n\n",
            i + 1, node.instance_name, ...
        ));
    }
    out
}
```

**测试模式** (silent_install.rs lines 86-166) — 针对内容断言，如：
```rust
#[test]
fn test_xml_contains_all_required_tags() {
    let content = generate_install_xml(&config).unwrap();
    assert!(content.contains("<INSTALL_PATH>"), "缺少 INSTALL_PATH 标签");
}
```

`dmmal_ini` 测试对应验证主备内容完全一致（Pitfall 1 防范）：
```rust
#[test]
fn test_dmmal_ini_same_for_both_nodes() {
    let ini = generate_dmmal_ini(&nodes);
    // 生成一次，SFTP 分发到两个节点用同一 bytes
    assert!(ini.contains("[MAL_INST1]"));
    assert!(ini.contains("[MAL_INST2]"));
}
```

`dmarch_ini` 测试验证主备 `ARCH_DEST` 方向相反（CLUS-01 验收）：
```rust
#[test]
fn test_dmarch_ini_primary_dest_is_standby_name() {
    let primary_ini = generate_dmarch_ini(&primary_node, "DMSVR02");
    assert!(primary_ini.contains("ARCH_DEST = DMSVR02"));
}
```

---

## Shared Patterns

### 错误处理
**来源:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/config/mod.rs` lines 1-3 和 `src/install/init.rs` lines 1-2

**应用于:** 所有新建 Rust 文件

```rust
// 顶层入口函数
use anyhow::{bail, Context, Result};

// 语义错误字符串格式（与项目中文消息保持一致）
bail!("配置验证失败: {} 无效: {}；有效值为 ...", field_name, value);

// 上下文链
.with_context(|| format!("无法读取配置文件: {}", path.display()))?

// 模块边界 thiserror 错误
#[derive(Debug, thiserror::Error)]
pub enum SshError { ... }
```

### tracing 进度日志
**来源:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/mod.rs` lines 26-44

**应用于:** `src/cluster/mod.rs`、`src/cluster/preflight.rs`、`src/cluster/deploy.rs`

```rust
// 步骤开始处打日志（在函数第一行）
tracing::info!("[N/M] 步骤名");

// 集群版加节点前缀（D-12 决策的日志格式）
tracing::info!("[node:{}][{}/{}] {}", node.role, step, total, step_name);
```

### TOML 反序列化
**来源:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/config/mod.rs` lines 7-65

**应用于:** `src/config/cluster.rs`

```rust
// 结构体三要素
#[derive(Debug, Deserialize)]
pub struct XxxConfig {
    #[serde(default = "default_yyy")]
    pub yyy: T,
}
fn default_yyy() -> T { ... }
impl Default for XxxConfig { fn default() -> Self { Self { yyy: default_yyy() } } }
```

敏感字段（SSH password）额外加 `#[serde(skip_serializing)]` 防日志泄漏（RESEARCH.md Security Domain）。

### clap 子命令嵌套
**来源:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/cli.rs` lines 17-60

**应用于:** `src/cli.rs` 新增的 `ClusterArgs`

```rust
// 嵌套子命令模式
#[derive(clap::Args)]
pub struct ClusterArgs {
    #[command(subcommand)]
    pub command: ClusterSubcommand,
}

#[derive(Subcommand)]
pub enum ClusterSubcommand {
    /// 部署主备集群
    Deploy(ClusterDeployArgs),
}

#[derive(clap::Args)]
pub struct ClusterDeployArgs {
    /// 集群 TOML 配置文件路径（必填）
    #[arg(long)]
    pub config: PathBuf,  // PathBuf 非 Option — 必填（D-05）
}
```

### tokio async 函数签名
**来源:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/mod.rs` line 25

**应用于:** `src/cluster/mod.rs`、`src/cluster/ssh.rs`、`src/cluster/preflight.rs`、`src/cluster/deploy.rs`、`src/cluster/health.rs`

```rust
pub async fn run(args: &ClusterDeployArgs) -> Result<()> {
    // ...
}
```

---

## No Analog Found

| 文件 | 角色 | 数据流 | 原因 |
|---|---|---|---|
| `src/cluster/ssh.rs` | service | request-response | 项目内无现有 SSH/russh 代码，使用 RESEARCH.md Pattern 1+2 |

---

## Metadata

**Analog 搜索范围:** `.claude/worktrees/agent-a693079c0c4cadfbf/src/`
**扫描文件数:** 7 个核心文件（cli.rs / config/mod.rs / config/validate.rs / install/mod.rs / install/silent_install.rs / install/checksum.rs / install/init.rs / main.rs）
**Pattern 提取日期:** 2026-06-12
