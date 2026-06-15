# Phase 4: 发布流水线 - Pattern Map

**Mapped:** 2026-06-13
**Files analyzed:** 8 个新建/修改文件
**Analogs found:** 8 / 8

---

## File Classification

| 新建/修改文件 | Role | Data Flow | 最近类比 | Match 质量 |
|---|---|---|---|---|
| `Cargo.toml` | config | transform | `Cargo.toml` 现有结构 | exact |
| `.cargo/config.toml` | config | transform | 无（新建） | no-analog |
| `.github/workflows/release.yml` | config (CI) | event-driven | `.github/workflows/update-versions.yml` | role-match |
| `src/cli.rs` | utility (CLI) | request-response | `src/cli.rs` 现有 `ClusterArgs`/`Commands` 结构 | exact |
| `src/main.rs` | utility (entry) | request-response | `src/main.rs` 现有 match 分支 | exact |
| `src/cluster/ssh.rs` | service | request-response | `src/cluster/ssh.rs` 自身（修复） | exact |
| `src/cluster/deploy.rs` | service | request-response | `src/cluster/deploy.rs` 自身（修复） | exact |
| `src/config/validate.rs` | utility | transform | `src/config/mod.rs` `validate_install_config()` | role-match |

---

## Pattern Assignments

### `Cargo.toml` — 新增 cargo-dist metadata + package 必填字段

**类比：** `Cargo.toml` 现有 `[package]` + `[dependencies]` 结构

**现有 package 块**（lines 1-8）：
```toml
[package]
name = "dm-database-installer"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "dm-installer"
path = "src/main.rs"
```

**需要新增的字段**（照 cargo-dist 要求，紧接现有 `[package]` 字段后追加）：
```toml
description = "达梦数据库安装器"
license = "MIT"
repository = "https://github.com/guangl/dm-database-installer"
```

**需要新增的 dist 配置块**（追加到文件末尾，在 `[dev-dependencies]` 之后）：
```toml
# 由 cargo dist init 生成，手动核对目标列表
[workspace.metadata.dist]
cargo-dist-version = "0.32.0"
ci = "github"
installers = ["shell", "powershell"]
targets = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
]

[workspace.metadata.dist.github-custom-runners]
aarch64-unknown-linux-gnu = "ubuntu-22.04"

[workspace.metadata.dist.dependencies.apt]
gcc-aarch64-linux-gnu = { version = "*", targets = ["aarch64-unknown-linux-gnu"] }

[profile.dist]
inherits = "release"
lto = "thin"
```

**关键约束：**
- Windows 目标必须用 `x86_64-pc-windows-msvc`，不能用 `windows-gnu`（ring/mio 不兼容）
- `repository` 字段是 cargo-dist 生成正确下载 URL 的前提

---

### `.cargo/config.toml` — aarch64 交叉编译 linker 配置（新建文件）

**类比：** 无现有类比（项目中 `.cargo/` 目录不存在）

**完整文件内容：**
```toml
# aarch64 交叉编译链接器配置
# 需要在 CI 中安装：apt-get install gcc-aarch64-linux-gnu
# 对应 [workspace.metadata.dist.dependencies.apt] 中的 gcc-aarch64-linux-gnu
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
```

---

### `.github/workflows/release.yml` — cargo-dist 生成的 Release CI

**类比：** `.github/workflows/update-versions.yml`（现有 CI 工作流结构）

**现有 update-versions.yml 关键结构**（lines 1-38）：
```yaml
name: update-versions
on:
  schedule:
    - cron: '0 1 * * *'
  workflow_dispatch:

jobs:
  update:
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
      - name: ...
        run: ...
```

**release.yml 的预期关键结构**（由 `cargo dist init` 生成，不要手写）：
```yaml
# 触发条件：v* tag push（D-02 锁定，不做 workflow_dispatch）
on:
  push:
    tags:
      - '**[0-9]+.[0-9]+.[0-9]+*'

jobs:
  plan:
    runs-on: ubuntu-22.04
    steps:
      - run: dist plan --output-format=json > plan-dist-manifest.json

  build-local-artifacts:
    needs: plan
    strategy:
      # 矩阵由 dist plan 动态生成，不静态写死
      matrix: ${{ fromJson(needs.plan.outputs.val).ci.github.artifacts_matrix }}
    runs-on: ${{ matrix.runner }}
    steps:
      # Windows MSVC 构建需要 NASM（ring crate 依赖）
      - name: Install NASM (Windows)
        if: runner.os == 'Windows'
        run: choco install nasm -y && echo "C:\Program Files\NASM" >> $env:GITHUB_PATH
        shell: pwsh
      # dist build 步骤由 cargo-dist 自动插入
```

**重要：** release.yml 必须通过 `cargo dist init` 命令生成，不要手写。Windows NASM 安装步骤需要在生成后手动追加到 `build-local-artifacts` job 的 steps 中。

---

### `src/cli.rs` — 新增 InstallWindows placeholder 子命令

**类比：** `src/cli.rs` 中现有 `ClusterArgs` 嵌套子命令模式（lines 64-84）

**现有 Commands enum 结构**（lines 17-30）：
```rust
/// 支持的子命令集合
#[derive(Subcommand)]
pub enum Commands {
    /// 安装达梦数据库单机实例
    Install(InstallArgs),
    /// 验证 TOML 配置文件合法性（不执行安装）
    Validate(ValidateArgs),
    /// 集群部署子命令
    Cluster(ClusterArgs),
    /// 生成 shell 补全脚本
    Completions {
        shell: clap_complete::Shell,
    },
}
```

**现有参数结构体模式**（lines 79-84，ClusterDeployArgs 作为参考）：
```rust
/// cluster deploy 子命令参数
#[derive(clap::Args)]
pub struct ClusterDeployArgs {
    /// 集群 TOML 配置文件路径（必填）
    #[arg(long)]
    pub config: PathBuf,
}
```

**新增模式**（照 ClusterArgs/InstallArgs 模式，插入 Commands enum）：
```rust
// 在 Commands enum 中新增（D-07 PLAT-04 placeholder）
/// 在 Windows 目标机上安装达梦（placeholder — PLAT-04 spike 待完成）
InstallWindows(InstallWindowsArgs),
```

```rust
/// install-windows 子命令参数（PLAT-04 placeholder）
#[derive(clap::Args)]
pub struct InstallWindowsArgs {
    /// TOML 配置文件路径（可选；未提供时使用内置默认参数）
    #[arg(long)]
    pub config: Option<PathBuf>,
}
```

**测试模式**（仿 test_install_args_defaults，lines 92-102）：
```rust
#[test]
fn test_install_windows_placeholder_parses() {
    let cli = Cli::try_parse_from(["dm-installer", "install-windows"]).unwrap();
    let Commands::InstallWindows(args) = cli.command else {
        panic!("expected InstallWindows command");
    };
    assert!(args.config.is_none(), "--config 应为 None");
}
```

---

### `src/main.rs` — 新增 InstallWindows match 分支

**类比：** `src/main.rs` 现有 match 分支（lines 26-39）

**现有 match 结构**（lines 26-39）：
```rust
match &cli_args.command {
    cli::Commands::Install(args) => install::run(args).await,
    cli::Commands::Validate(args) => config::validate::run(args),
    cli::Commands::Cluster(args) => match &args.command {
        cli::ClusterSubcommand::Deploy(deploy_args) => cluster::run(deploy_args).await,
    },
    cli::Commands::Completions { shell } => {
        use clap::CommandFactory;
        use clap_complete::generate;
        let mut cmd = cli::Cli::command();
        generate(*shell, &mut cmd, "dm-installer", &mut std::io::stdout());
        Ok(())
    }
}
```

**新增分支**（RESEARCH.md Pattern 5 推荐的友好错误输出，D-07）：
```rust
cli::Commands::InstallWindows(_args) => {
    // PLAT-04 spike: setup.exe /q /XML <path> 集成待完成
    // DM Windows 安装包 URL 需从 eco.dameng.com 单独验证
    eprintln!("[WARN] Windows 目标机安装尚未实现（PLAT-04 spike 待完成）");
    eprintln!("请参考: https://eco.dameng.com/ 手动获取 Windows 安装包");
    std::process::exit(1);
}
```

**选择 `eprintln!` + `exit(1)` 而非 `todo!()`**：避免正常路径 panic，用户体验更友好（RESEARCH.md Pattern 5 明确推荐）。

---

### `src/cluster/ssh.rs` — CR-02/CR-03/CR-05 三处修复

**类比：** `src/cluster/ssh.rs` 自身（修复现有代码）

**CR-02 修复：sftp_write 改用 `sftp.create()` + `write_all`**

现有问题代码（lines 170-176）：
```rust
sftp.write(remote_path, bytes)
    .await
    .map_err(|source| SshError::SftpUpload {
        remote_path: remote_path.to_string(),
        source,
    })
```

修复模式（从 SshError 错误结构和现有 `map_err` 模式提取，lines 24-29 + lines 164-169）：
```rust
let mut remote_file = sftp
    .create(remote_path)
    .await
    .map_err(|source| SshError::SftpUpload {
        remote_path: remote_path.to_string(),
        source,
    })?;
use tokio::io::AsyncWriteExt;
remote_file
    .write_all(bytes)
    .await
    .map_err(|io_err| SshError::SftpUpload {
        remote_path: remote_path.to_string(),
        source: russh_sftp::client::error::Error::UnexpectedBehavior(
            io_err.to_string(),
        ),
    })
```

**CR-03 修复：新增 `expand_tilde` 函数**

插入位置：`try_key_auth` 函数前（line 117 前），调用点在 line 122（`load_secret_key` 调用）。

模式来源：`src/install/silent_install.rs` 的 `xml_escape` 工具函数风格（lines 76-84）：
```rust
/// 展开路径中的 `~` 前缀为 $HOME 环境变量值。
/// `~` 对 Rust PathBuf 是字面字符，不会自动展开。
fn expand_tilde(path: &std::path::PathBuf) -> std::path::PathBuf {
    if let Some(path_str) = path.to_str() {
        if let Some(rest) = path_str.strip_prefix("~/") {
            if let Some(home_dir) = std::env::var_os("HOME") {
                return std::path::PathBuf::from(home_dir).join(rest);
            }
        }
    }
    path.clone()
}
```

在 `try_key_auth` 中调用：
```rust
async fn try_key_auth(
    handle: &mut client::Handle<TofuHandler>,
    user: &str,
    identity_file: &std::path::PathBuf,
) -> Result<(), russh::Error> {
    let expanded_path = expand_tilde(identity_file);  // CR-03 fix
    let key_pair = load_secret_key(&expanded_path, None)?;
    // 其余不变...
}
```

**CR-05 修复：TOFU `check_server_key` 新增指纹日志**

现有问题代码（lines 48-58）：
```rust
async fn check_server_key(
    &mut self,
    server_public_key: &russh::keys::PublicKey,
) -> Result<bool, russh::Error> {
    self.accepted_keys
        .lock()
        .unwrap()
        .push(server_public_key.clone());
    Ok(true)
}
```

修复模式（仿 `src/cluster/deploy.rs` tracing::warn! 风格，lines 32/107/146）：
```rust
async fn check_server_key(
    &mut self,
    server_public_key: &russh::keys::PublicKey,
) -> Result<bool, russh::Error> {
    let fingerprint = server_public_key.fingerprint(Default::default());
    tracing::warn!(
        "[ssh][TOFU] 接受服务器公钥（未验证）: {} — 生产环境请配置 host_key_fingerprint",
        fingerprint
    );
    match self.accepted_keys.lock() {
        Ok(mut accepted) => accepted.push(server_public_key.clone()),
        Err(poisoned) => poisoned.into_inner().push(server_public_key.clone()),
    }
    Ok(true)
}
```

**CR-02/CR-03/CR-05 测试模式**（仿 lines 282-337 现有 `#[cfg(test)] mod tests` 结构）：
```rust
#[test]
fn test_expand_tilde_replaces_home() {
    std::env::set_var("HOME", "/home/testuser");
    let input = std::path::PathBuf::from("~/.ssh/id_rsa");
    let expanded = expand_tilde(&input);
    assert_eq!(expanded, std::path::PathBuf::from("/home/testuser/.ssh/id_rsa"));
}

#[test]
fn test_expand_tilde_no_tilde_unchanged() {
    let input = std::path::PathBuf::from("/absolute/path/key");
    let expanded = expand_tilde(&input);
    assert_eq!(expanded, input);
}
```

---

### `src/cluster/deploy.rs` — CR-01/CR-04 两处修复

**类比：** `src/cluster/deploy.rs` 自身（修复现有代码）

**CR-01 修复：安装包改用 `.bin` 可执行文件路径**

现有问题代码（lines 45-54）：
```rust
let remote_iso = format!("/tmp/dm_installer_{}.iso", node.instance_name);
runner
    .sftp_write(&remote_iso, &bytes)
    .await
    .context("SFTP 上传安装包失败")?;
let install_cmd = format!("cd /tmp && DMInstall.bin -q {}", remote_xml);
```

修复模式（使用可执行路径变量，`chmod +x` 后按路径执行）：
```rust
let remote_bin_path = format!("/tmp/dm_installer_{}.bin", node.instance_name);
runner
    .sftp_write(&remote_bin_path, &bytes)
    .await
    .context("SFTP 上传安装包失败")?;
runner
    .exec(&format!("chmod +x {}", remote_bin_path))
    .await
    .map_err(|e| anyhow::anyhow!("chmod 安装包失败: {}", e))?;
let install_cmd = format!("{} -q {}", remote_bin_path, remote_xml);
```

**CR-04 修复：新增 `shell_quote` 函数防命令注入**

模式来源：`src/install/silent_install.rs` 的 `xml_escape` 函数风格（lines 76-84）：
```rust
/// 对 shell 参数进行单引号转义，防止命令注入。
/// 所有路径和实例名在拼入 shell 命令前必须经过此函数。
fn shell_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "'\\''"))
}
```

使用点（`build_dminit_args`、`start_dmserver_mount`、`configure_database_role` 中路径拼接处）：
```rust
// build_dminit_args 修复示例
format!("PATH={}", shell_quote(&node.data_path)),
format!("INSTANCE_NAME={}", shell_quote(&node.instance_name)),
```

```rust
// start_dmserver_mount 修复示例
let install_path = shell_quote(&node.install_path);
let data_path = shell_quote(&node.data_path);
let instance_name = shell_quote(&node.instance_name);
let cmd = format!(
    "nohup {0}/bin/dmserver {1}/{2}/dm.ini mount > /tmp/dmserver_{2}.log 2>&1 &",
    install_path, data_path, instance_name
);
```

**CR-01/CR-04 测试模式**（仿 `test_build_dminit_args_format` 和 `test_upload_installer_and_install_pushes_xml`，lines 255-342）：
```rust
#[test]
fn test_shell_quote_single_quotes_path() {
    assert_eq!(shell_quote("/opt/dmdbms"), "'/opt/dmdbms'");
}

#[test]
fn test_shell_quote_escapes_embedded_single_quote() {
    assert_eq!(shell_quote("it's"), "'it'\\''s'");
}

#[tokio::test]
async fn test_upload_installer_uses_bin_extension() {
    let node = make_primary_node();
    let runner = MockRunner::new(vec![]);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let _ = upload_installer_and_install(&node, tmp.path(), &runner).await;
    let sftp_log = runner.sftp_log();
    let has_bin = sftp_log.iter().any(|(path, _)| path.ends_with(".bin"));
    assert!(has_bin, "sftp_log 应含 .bin 路径（CR-01）: {:?}", sftp_log.iter().map(|(p,_)| p).collect::<Vec<_>>());
    let has_iso = sftp_log.iter().any(|(path, _)| path.ends_with(".iso"));
    assert!(!has_iso, "sftp_log 不应含 .iso 路径（CR-01 修复后）");
}
```

---

### `src/config/validate.rs` — CR-04 路径字符集白名单校验（可选）

**类比：** `src/config/mod.rs` 的 `validate_install_config()` 函数（lines 95-117）

**现有路径值域校验模式**（lines 95-117）：
```rust
pub fn validate_install_config(cfg: &InstallConfig) -> Result<()> {
    if ![4u8, 8, 16, 32].contains(&cfg.page_size) {
        bail!(
            "配置验证失败: page_size 无效: {}；有效值为 4/8/16/32",
            cfg.page_size
        );
    }
    // ... 其他字段校验
    Ok(())
}
```

**新增路径字符集白名单校验**（CR-04 防注入的配置层方案，在 `validate_install_config` 中追加）：
```rust
/// 验证路径字段只包含安全字符（防 shell 注入）。
fn validate_safe_path(field_name: &str, value: &str) -> Result<()> {
    let is_safe = value
        .chars()
        .all(|ch| ch.is_alphanumeric() || "/\\-_.".contains(ch));
    if !is_safe {
        bail!(
            "配置验证失败: {} 包含非法字符: {}；只允许 [a-zA-Z0-9/\\-_.]",
            field_name,
            value
        );
    }
    Ok(())
}
```

在 `validate_install_config` 末尾追加三次调用：
```rust
validate_safe_path("install_path", &cfg.install_path)?;
validate_safe_path("data_path", &cfg.data_path)?;
validate_safe_path("instance_name", &cfg.instance_name)?;
```

---

## Shared Patterns

### Tracing 日志模式
**来源：** `src/cluster/deploy.rs` lines 32、107、146 的 `tracing::info!`/`tracing::warn!` 用法
**适用范围：** CR-05 TOFU 修复、CR-01 安装步骤日志
```rust
tracing::info!("[node:{:?}][步骤] 操作描述", node.role);
tracing::warn!("[模块][WARN] 安全相关警告信息");
```

### anyhow 错误链模式
**来源：** `src/cluster/deploy.rs` lines 34/39/44/50 的 `.context()`/`.map_err()` 用法
**适用范围：** 所有修复函数的错误处理
```rust
runner
    .exec(&cmd)
    .await
    .map_err(|e| anyhow::anyhow!("操作描述失败: {}", e))?;
```

### thiserror 错误类型模式
**来源：** `src/cluster/ssh.rs` lines 13-29 的 `SshError` enum
**适用范围：** 新错误 variant（若需要，CR-02 不需要新 variant）
```rust
#[derive(Debug, Error)]
pub enum SshError {
    #[error("描述 {field}: {source}")]
    VariantName {
        field: String,
        #[source]
        source: 底层错误类型,
    },
}
```

### clap derive CLI 模式
**来源：** `src/cli.rs` lines 17-84 全文
**适用范围：** PLAT-04 InstallWindows 子命令
```rust
/// 文档注释成为 --help 文本
#[derive(clap::Args)]
pub struct XxxArgs {
    #[arg(long)]
    pub field: Option<PathBuf>,
}
```

### 测试结构模式
**来源：** `src/cli.rs` lines 86-215、`src/cluster/deploy.rs` lines 203-343
**适用范围：** 所有新增/修复代码的测试
```rust
#[cfg(test)]
mod tests {
    use super::*;
    // 单元测试：纯函数用 #[test]，async 用 #[tokio::test]
    // MockRunner 注入：用 MockRunner::new(responses) 替代真实 SSH
    // 断言语言：中文描述，含实际值（"应含 X，实际: {msg}"）
}
```

---

## No Analog Found

| 文件 | Role | Data Flow | 原因 |
|---|---|---|---|
| `.cargo/config.toml` | config | transform | 项目中无现有 `.cargo/` 目录，无类比；内容由 RESEARCH.md Pattern 2 指定 |
| `.github/workflows/release.yml` | config (CI) | event-driven | 由 `cargo dist init` 命令生成，不应手写；`update-versions.yml` 仅提供结构参考，不是内容模板 |

---

## Metadata

**搜索范围：** `src/` 全部 `.rs` 文件、`.github/workflows/`、`Cargo.toml`
**扫描文件数：** 23 个 Rust 文件 + 2 个 YAML 工作流 + 1 个 Cargo.toml
**Pattern 提取日期：** 2026-06-13
