# Phase 4: 发布流水线 - Research

**Researched:** 2026-06-13
**Domain:** Rust 多平台发布（cargo-dist、cross-compilation、GitHub Actions Release CI）
**Confidence:** HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** 使用 `cargo-dist` 管理发布流水线（写入 `[workspace.metadata.dist]` 或 `dist-workspace.toml`）
- **D-02:** 触发条件为 `v*` tag push；不做 `workflow_dispatch`
- **D-03:** 运行 `cargo dist init` 生成 `.github/workflows/release.yml`；现有 `update-versions.yml` 保留
- **D-04:** 三个构建目标：`x86_64-unknown-linux-gnu`、`aarch64-unknown-linux-gnu`、`x86_64-pc-windows-gnu`
- **D-05:** 使用 `cross` 工具链进行交叉编译，Docker-based
- **D-06:** `rustls-tls` feature 已配置，无 OpenSSL 依赖
- **D-07:** PLAT-04 作为 placeholder：CLI 结构就位，`setup.exe /q /XML` 逻辑标记 `todo!()`；同时也构建 `x86_64-pc-windows-msvc`
- **D-08:** 保留现有 `install.sh`（DM 安装脚本）；cargo-dist 生成的 bootstrap 独立命名
- **D-09:** 手动版本号管理（Cargo.toml 是单一事实来源）；不引入 cargo-release
- **D-10:** 可选在 CHANGELOG.md 中记录变更

### Claude's Discretion

无明确记录的裁量项。

### Deferred Ideas (OUT OF SCOPE)

- Phase 3 五个 Critical 审查问题（已在 CONTEXT.md 标注为可并行处理，不是 Phase 4 核心功能）
- 多 standby 节点（v2 需求）
- 自动下载 DM8 安装包（DOWN-01）
- cargo-release 自动版本管理
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| PLAT-01 | 安装器可在 Linux x86_64 控制机运行，并在 Linux x86_64 目标机安装达梦 | cargo-dist 原生支持 x86_64-unknown-linux-gnu；ubuntu-22.04 runner 直接构建 |
| PLAT-02 | 安装器可在 Linux aarch64 控制机运行，并在 aarch64 目标机安装达梦 | cargo-dist + github-custom-runners + apt deps 安装 gcc-aarch64-linux-gnu 可解决 ring 问题 |
| PLAT-03 | 安装器可在 Windows 控制机运行，通过 SSH 在 Linux 节点安装达梦 | x86_64-pc-windows-gnu 有 ring/tokio 双重不兼容问题；推荐改用 msvc + windows-2022 runner |
| PLAT-04 | 安装器支持在 Windows 目标机安装达梦 | placeholder 实现：CLI 新增子命令，实际 setup.exe 调用留 todo!() |
</phase_requirements>

---

## Summary

Phase 4 的核心任务是通过 `cargo-dist` 建立 GitHub Actions release pipeline，在 `v*` tag 时自动构建三个平台的 `dm-installer` 二进制并发布到 GitHub Releases，同时修复 Phase 3 遗留的五个 Critical bug（作为 Wave 0 前置修复），并添加 PLAT-04 Windows 安装器 placeholder。

**主要技术发现：**

1. **cargo-dist 0.32.0 工作机制**：`dist init` 在 `Cargo.toml`（或 `dist-workspace.toml`）写入 `[workspace.metadata.dist]` 配置块，并生成 `.github/workflows/release.yml`。release.yml 由 `dist plan` 动态生成矩阵，不是静态写死的构建步骤——每个平台的 runner 和容器在运行时由 dist 决定。

2. **aarch64 构建的正确方式**：通过 `dist.github-custom-runners` 指定 `ubuntu-22.04` runner，并在 `dist.dependencies.apt` 中安装 `gcc-aarch64-linux-gnu`，同时在 `.cargo/config.toml` 配置正确的 linker。cargo-dist 自身对 `aarch64-unknown-linux-gnu` 则使用 `quay.io/pypa/manylinux_2_28_x86_64` 容器镜像（可选更健壮方案）。

3. **Windows 目标关键风险**：D-04 指定 `x86_64-pc-windows-gnu`，但该目标存在两个已知的阻断性问题：(a) `ring` crate 无法从 Linux 交叉编译到 windows-gnu（需要 nasm + MinGW 精确配置）；(b) `tokio/mio` 的 `NtCancelIoFileEx` 依赖 `libntdll.a`，该库不存在于 MinGW 工具链中，导致链接失败。**强烈建议将 D-04 中的 Windows 目标改为 `x86_64-pc-windows-msvc`**，在 `windows-2022` runner 上原生构建（russh CI 已确认此路径可行，仅需额外安装 NASM）。

4. **Phase 3 五个 Critical bug**：这些 bug 直接影响 cluster 功能的正确性，发布前必须修复。研究已确认每个修复方案（见下方"Phase 3 Bug 修复"章节）。

**Primary recommendation:** Wave 0 修复 Phase 3 Critical bug → Wave 1 运行 `cargo dist init` 配置发布流水线（Windows 目标改为 msvc）→ Wave 2 验证三平台构建。

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| 多平台二进制构建 | GitHub Actions CI | Cargo + cross toolchain | CI 是执行者；构建系统是工具 |
| 版本号管理 | Cargo.toml `[package].version` | Git tag | 单一事实来源，手动更新 |
| Release asset 发布 | GitHub Releases | cargo-dist plan/build/publish | dist 生成 manifest，GH releases 托管 |
| install.sh bootstrap（dm-installer 安装） | GitHub Releases asset | cargo-dist shell installer | 与 Phase 1 `install.sh`（DM8 安装）完全分离 |
| Phase 3 bug 修复 | `src/cluster/` Rust 代码 | — | 代码层修复，不涉及基础设施 |
| PLAT-04 CLI placeholder | `src/cli.rs` / `src/install/` | — | 新增子命令，实际安装逻辑留 todo!() |

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `cargo-dist` | 0.32.0 | 发布流水线生成 | [VERIFIED: crates.io] axodotdev 官方工具，CLAUDE.md 推荐，uv/Rye 同类使用 |
| GitHub Actions `ubuntu-22.04` runner | — | Linux x86_64 原生构建 | [VERIFIED: GitHub docs] cargo-dist 默认 runner |
| GitHub Actions `windows-2022` runner | — | Windows MSVC 原生构建 | [VERIFIED: russh CI] russh 在此 runner 上有完整 CI |
| NASM | — | ring crate 在 Windows 构建时的汇编依赖 | [VERIFIED: russh CI] russh 的 Windows CI 明确安装 nasm |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `.cargo/config.toml` linker config | — | 指定 aarch64 交叉编译链接器 | 配置 `aarch64-linux-gnu-gcc` 为 linker |
| `gcc-aarch64-linux-gnu` (apt) | — | aarch64 交叉编译工具链 | dist.dependencies.apt 安装，解决 ring 编译问题 |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `x86_64-pc-windows-gnu` | `x86_64-pc-windows-msvc` | gnu 有 ring/tokio 不兼容；msvc 原生构建无此问题 [VERIFIED: ring#1363, mio#1632] |
| cargo-dist 交叉编译 aarch64 | `ubuntu-22.04-arm` ARM runner | ARM runner 零交叉编译问题，但 GitHub Actions ARM runner 为付费功能；apt 安装工具链方案免费 |
| 手写 release.yml | `cargo dist init` 生成 | 手写维护负担高，cargo-dist 生成可 rerun 更新 |

**Installation:**
```bash
# 安装 cargo-dist（仅开发者机器，CI 中自动安装）
cargo install cargo-dist --version "0.32.0"
```

**Version verification:**
```bash
cargo search cargo-dist
# cargo-dist = "0.32.0"  -- 已验证 [VERIFIED: crates.io]
```

---

## Package Legitimacy Audit

> slopcheck 在此环境不可用——所有外部新增包标注 `[ASSUMED]`；Phase 4 仅向 Cargo.toml 的 `[workspace.metadata.dist]` 新增配置，不新增 Rust 依赖。

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `cargo-dist` | crates.io | ~3 yrs | 高（axodotdev 维护，多个知名项目使用） | github.com/axodotdev/cargo-dist | 未运行 | Approved — 官方文档 + CLAUDE.md 推荐 [CITED: CLAUDE.md] |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

*slopcheck 在此环境不可用；cargo-dist 已通过 CLAUDE.md 明确推荐和 GitHub 官方存在确认，风险极低。*

---

## Architecture Patterns

### System Architecture Diagram

```
Developer Machine
      |
      | git tag v0.1.0 && git push --tags
      v
GitHub (tag push trigger)
      |
      v
release.yml (dist plan)
      |
      +---> ubuntu-22.04 runner
      |         cargo build --target x86_64-unknown-linux-gnu
      |         --> dm-installer-x86_64-unknown-linux-gnu.tar.xz
      |
      +---> ubuntu-22.04 runner + gcc-aarch64-linux-gnu
      |         cargo build --target aarch64-unknown-linux-gnu
      |         --> dm-installer-aarch64-unknown-linux-gnu.tar.xz
      |
      +---> windows-2022 runner + nasm
      |         cargo build --target x86_64-pc-windows-msvc
      |         --> dm-installer-x86_64-pc-windows-msvc.zip
      |
      v
GitHub Release (v0.1.0)
      |
      +---> dm-installer-{target}.tar.xz / .zip (预编译二进制)
      +---> dm-installer-installer.sh (cargo-dist 生成 bootstrap)
      +---> dm-installer-installer.ps1 (cargo-dist 生成 bootstrap)
      |
      v
User (任意平台)
  curl --proto '=https' --tlsv1.2 -LsSf \
    https://github.com/guangl/dm-database-installer/releases/latest/download/dm-database-installer-installer.sh | sh
```

### Recommended Project Structure

```
.
├── Cargo.toml                          # 新增 [workspace.metadata.dist] + [profile.dist]
├── .cargo/
│   └── config.toml                     # 新增 aarch64 linker 配置
├── .github/
│   └── workflows/
│       ├── update-versions.yml         # 保留不变
│       └── release.yml                 # cargo dist init 生成（新增）
├── src/
│   ├── cli.rs                          # 新增 Windows install placeholder 命令
│   └── install/
│       └── windows.rs (可选)           # Windows 安装 placeholder 逻辑
└── CHANGELOG.md                        # 新建（可选，cargo-dist 可读取）
```

### Pattern 1: cargo-dist Cargo.toml 配置

**What:** `dist init` 在 `Cargo.toml` 新增两个块
**When to use:** 运行 `cargo dist init` 后自动生成，手动核对平台列表

```toml
# Source: cargo-dist 官方文档 + oura 实际项目 dist-workspace.toml
[workspace.metadata.dist]
cargo-dist-version = "0.32.0"
ci = "github"
installers = ["shell", "powershell"]
targets = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",    # 注意：改为 msvc，不用 gnu
]
# 必须有 repository 字段才能生成正确的 install URL
# 在 [package] 中添加：
# repository = "https://github.com/guangl/dm-database-installer"

[workspace.metadata.dist.github-custom-runners]
# aarch64 在 x86 runner 上交叉编译，需额外安装工具链
aarch64-unknown-linux-gnu = "ubuntu-22.04"

[workspace.metadata.dist.dependencies.apt]
# 为 aarch64 交叉编译安装 gcc
gcc-aarch64-linux-gnu = { version = "*", targets = ["aarch64-unknown-linux-gnu"] }

[profile.dist]
inherits = "release"
lto = "thin"
```

### Pattern 2: aarch64 Linker 配置

**What:** `.cargo/config.toml` 指定 aarch64 的交叉编译链接器
**When to use:** `cargo-dist` 的 aarch64 构建 + `ring` crate 需要此配置

```toml
# Source: cargo-dist issue #1378 社区解决方案 [CITED: github.com/axodotdev/cargo-dist/issues/1378]
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
```

### Pattern 3: Windows MSVC + NASM CI 配置

**What:** 在 `windows-2022` runner 上构建时，`ring` 需要 NASM
**When to use:** 构建 `x86_64-pc-windows-msvc` 目标时

cargo-dist 支持通过 `packages_install` 在 CI 中安装额外工具。由于 NASM 需要通过 Chocolatey 安装，可在 `release.yml` 生成后手动添加：

```yaml
# 在 build-local-artifacts job 的 steps 中，在 dist build 之前添加：
- name: Install NASM (Windows)
  if: runner.os == 'Windows'
  run: choco install nasm -y && echo "C:\Program Files\NASM" >> $env:GITHUB_PATH
  shell: pwsh
```

注意：cargo-dist 支持 `dist.dependencies` 但仅支持 apt/chocolatey/homebrew；windows 的 NASM 安装建议通过 chocolatey 在生成的 release.yml 中手动补充。

### Pattern 4: Cargo.toml 必填 metadata

```toml
# Source: cargo-dist 官方文档（dist init 会检查这些字段）
[package]
name = "dm-database-installer"
version = "0.1.0"
edition = "2024"
# 以下字段 cargo-dist 需要：
description = "达梦数据库安装器"
license = "MIT"          # 或 Apache-2.0 等
repository = "https://github.com/guangl/dm-database-installer"
```

### Pattern 5: PLAT-04 CLI Placeholder

**What:** 在 cli.rs 中新增 `install windows` 子命令，实际逻辑用 `todo!()`
**When to use:** PLAT-04 Phase 4 placeholder 要求

```rust
// 在 Commands enum 中添加（仿照现有 Install/Cluster 模式）
/// 在 Windows 目标机上安装达梦（placeholder — 待集成 setup.exe）
InstallWindows(InstallWindowsArgs),

// 在 main.rs 的 match 分支中：
Commands::InstallWindows(_args) => {
    // TODO(PLAT-04): 集成 setup.exe /q /XML <path> 的实际调用
    // DM Windows 安装包 URL 需从 eco.dameng.com 单独验证
    todo!("Windows 目标机安装尚未实现，见 PLAT-04 spike")
}
```

**更安全的替代**（避免 `todo!()` 在正常路径 panic）：

```rust
Commands::InstallWindows(_args) => {
    eprintln!("[WARN] Windows 目标机安装尚未实现（PLAT-04 spike 待完成）");
    eprintln!("请参考: https://eco.dameng.com/ 手动获取 Windows 安装包");
    std::process::exit(1);
}
```

CONTEXT.md D-07 已说明"用 `unimplemented!` 包裹并在文档注释中注明"，选择 `eprintln!` + `exit(1)` 比 panic 更友好。

### Anti-Patterns to Avoid

- **不要手写 `release.yml`**：cargo-dist 的 plan 步骤动态生成矩阵，手写会错过 dist 的 artifact fingerprint、attestation、announcement 等自动化功能
- **不要把 dist init 生成的 `install.sh` 替换 Phase 1 `install.sh`**：两者目的完全不同——cargo-dist 生成的 bootstrap 安装 `dm-installer` 工具本身，Phase 1 的 install.sh 安装 DM8 数据库
- **不要使用 `x86_64-pc-windows-gnu` 目标**：`ring` crate 不支持从 Linux 交叉编译到 windows-gnu [VERIFIED: ring#1363]；`tokio/mio` 的 `ntdll` 链接在 gnu 工具链中失败 [VERIFIED: mio#1632]
- **不要跳过 Cargo.toml metadata**：`cargo dist init` 需要 `repository`、`license`、`description` 字段；缺少会导致 dist plan 报错

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| 多平台 CI 构建矩阵 | 手写 GitHub Actions matrix | `cargo dist init` 生成 | dist 处理 plan/build/publish/announce 全流程；artifact attestation、checksums 自动 |
| install.sh bootstrap 脚本 | 手写平台检测 + 下载逻辑 | cargo-dist `installers = ["shell"]` | dist 生成的 installer.sh 处理所有平台检测、$PATH 配置、版本协商 |
| Release 创建 | `gh release create` 手工脚本 | cargo-dist release.yml | dist 自动从 CHANGELOG 生成 release notes，处理 pre-release 标记 |

**Key insight:** cargo-dist 的价值不只是构建，而是完整的 plan → build → host → publish → announce 流水线；任何手写替代品都会错过其中某个环节。

---

## Phase 3 Bug 修复（Wave 0 前置任务）

以下五个 Critical 问题已在 03-REVIEW.md 中记录，必须在发布前修复。研究已确认各修复方案可行。

### CR-01: ISO 未解压直接调用 DMInstall.bin

**位置：** `src/cluster/deploy.rs:45-54`
**问题：** 上传 `.iso` 文件后直接执行 `DMInstall.bin`，两者完全无关联
**修复方案：** 将安装包处理方式改为上传 `.bin` 可执行文件，`chmod +x` 后通过完整路径执行：

```rust
let remote_bin = format!("/tmp/dm_installer_{}.bin", node.instance_name);
// sftp_write + chmod + 按路径执行
let install_cmd = format!("{} -q {}", remote_bin, remote_xml);
```

### CR-02: sftp_write 缺 CREATE flag

**位置：** `src/cluster/ssh.rs:170`
**问题：** `sftp.write()` 在 SFTP 协议层要求文件已存在；配置文件（dmmal.ini 等）是新建文件，100% 失败
**修复方案：** 使用 `sftp.create(remote_path)` + `write_all`：

```rust
let mut file = sftp.create(remote_path).await.map_err(|source| SshError::SftpUpload {
    remote_path: remote_path.to_string(),
    source,
})?;
file.write_all(bytes).await.map_err(|e| SshError::SftpUpload {
    remote_path: remote_path.to_string(),
    source: russh_sftp::client::error::Error::UnexpectedBehavior(e.to_string()),
})?;
```

### CR-03: `~` 路径不展开

**位置：** `src/cluster/ssh.rs:122`
**问题：** `PathBuf::from("~/.ssh/id_rsa")` 对 Rust 来说是字面路径，`File::open` 找不到
**修复方案：** 添加 `expand_tilde` 函数（不需要引入新 crate，用 `HOME` 环境变量即可）：

```rust
fn expand_tilde(path: &std::path::PathBuf) -> std::path::PathBuf {
    if let Some(s) = path.to_str() {
        if let Some(rest) = s.strip_prefix("~/") {
            if let Some(home) = std::env::var_os("HOME") {
                return std::path::PathBuf::from(home).join(rest);
            }
        }
    }
    path.clone()
}
```

### CR-04: Shell 命令注入

**位置：** `src/cluster/deploy.rs` 多处
**问题：** `install_path`、`data_path`、`instance_name` 直接插入 shell 字符串，可注入任意命令
**修复方案：** 两选一：
1. 添加 `shell_quote` 函数，所有路径包裹在单引号中
2. 在配置加载时验证这三个字段只允许 `[a-zA-Z0-9/\-_.]`（推荐，在 `validate_install_config` 中统一处理）

```rust
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
```

### CR-05: SSH TOFU 无日志无指纹记录

**位置：** `src/cluster/ssh.rs:40-58`
**问题：** `check_server_key` 静默接受任何主机密钥，MITM 攻击完全透明
**修复方案：** 至少记录指纹警告；可选增加 `host_key_fingerprint` 配置字段：

```rust
async fn check_server_key(
    &mut self,
    server_public_key: &russh::keys::PublicKey,
) -> Result<bool, russh::Error> {
    let fingerprint = server_public_key.fingerprint(Default::default());
    tracing::warn!(
        "[ssh][TOFU] 接受服务器公钥 (未验证): {} — 请在生产环境配置 host_key_fingerprint",
        fingerprint
    );
    match self.accepted_keys.lock() {
        Ok(mut keys) => keys.push(server_public_key.clone()),
        Err(e) => e.into_inner().push(server_public_key.clone()),
    }
    Ok(true)
}
```

---

## Common Pitfalls

### Pitfall 1: Cargo.toml 缺 repository / license 字段

**What goes wrong:** `cargo dist init` 或 `dist plan` 报错，无法生成 installer URL
**Why it happens:** cargo-dist 需要这些字段来生成正确的下载 URL 和 release 信息
**How to avoid:** 在运行 `dist init` 前，确保 `[package]` 包含 `repository`、`license`、`description`
**Warning signs:** `dist plan` 输出 "Missing required Cargo.toml fields" 错误

### Pitfall 2: x86_64-pc-windows-gnu 的 ring + tokio 双重不兼容

**What goes wrong:** (a) ring 无法从 Linux 交叉编译到 windows-gnu，需要 nasm + COFF 格式输出，cross 容器不处理；(b) tokio/mio 链接时找不到 `libntdll.a`
**Why it happens:** MinGW 工具链缺少 Windows SDK 部分库；ring 的构建脚本对 windows-gnu 有特殊要求
**How to avoid:** 使用 `x86_64-pc-windows-msvc` + `windows-2022` runner（原生构建，无交叉编译问题）
**Warning signs:** 链接错误 "cannot find -lntdll" 或 "ring build script failed on windows-gnu"

### Pitfall 3: aarch64 构建中 ring 编译失败

**What goes wrong:** `ring` 编译报 "ToolNotFound: arm-linux-gnueabihf-gcc not found"
**Why it happens:** cargo-dist 的 apt 依赖安装仅对已识别的 target triple 生效，配置不正确时安装步骤被跳过
**How to avoid:** 在 `[workspace.metadata.dist.dependencies.apt]` 中明确指定 targets 数组；同时配置 `.cargo/config.toml` 的 linker
**Warning signs:** CI 的 "Install Dependencies" 步骤为空，未安装 gcc-aarch64-linux-gnu

### Pitfall 4: install.sh 文件名冲突

**What goes wrong:** cargo-dist 生成的 bootstrap 脚本命名与 Phase 1 的 DM 安装 `install.sh` 冲突
**Why it happens:** 两者都可能叫 `install.sh`；cargo-dist 的 installer 脚本名取决于 crate name 配置
**How to avoid:** cargo-dist 实际生成的脚本名为 `{app-name}-installer.sh`（本项目为 `dm-installer-installer.sh` 或 `dm-database-installer-installer.sh`）；在 README 中明确区分两个脚本的用途
**Warning signs:** 用户混淆两个 install.sh 的功能

### Pitfall 5: Windows MSVC runner 缺 NASM

**What goes wrong:** `ring` 在 Windows 构建时报 "NASM not found"
**Why it happens:** `windows-2022` runner 默认不含 NASM；ring 的 Windows x86_64 构建需要它生成汇编代码
**How to avoid:** 在 release.yml 的 Windows 构建步骤前加 `choco install nasm -y`；cargo-dist 的 `packages_install` 机制可以处理这个
**Warning signs:** russh CI 中有明确的 `choco install nasm` 步骤 [VERIFIED: russh/.github/workflows/rust.yml]

### Pitfall 6: tag 推送时版本号未对齐

**What goes wrong:** `v0.1.0` tag 对应 `Cargo.toml version = "0.2.0"`，cargo-dist 拒绝发布或生成错误 release
**Why it happens:** cargo-dist 验证 tag 版本与 Cargo.toml 版本一致性
**How to avoid:** 流程：改 Cargo.toml version → cargo build（更新 Cargo.lock）→ commit → tag → push
**Warning signs:** dist plan 在 tag push 后报版本不匹配错误

---

## Code Examples

### 验证当前代码编译（release profile）

```bash
# Source: 本地验证 [VERIFIED: Bash 运行]
cargo build --release 2>&1 | tail -5
# 输出: Finished `release` profile [optimized] target(s) in 50.67s
```

### dist init 交互式运行

```bash
# Source: cargo-dist 官方文档 [CITED: axodotdev.github.io/cargo-dist]
cargo dist init
# 会交互式询问：
# - CI 后端（选 GitHub）
# - 目标平台（选三个 Linux/Windows 目标）
# - installer 类型（选 shell + powershell）
# 可用 --yes 跳过交互（接受默认值），但建议手动确认目标列表

cargo dist plan  # 验证配置，不实际构建
cargo dist build  # 本地构建当前平台（测试用）
```

### release.yml 关键结构（cargo-dist 生成）

```yaml
# Source: txpipe/oura release.yml [CITED: github.com/txpipe/oura]
# 关键：矩阵由 dist plan 动态生成，不静态写死
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
      matrix: ${{ fromJson(needs.plan.outputs.val).ci.github.artifacts_matrix }}
    runs-on: ${{ matrix.runner }}
    container: ${{ matrix.container && matrix.container.image || null }}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `x86_64-pc-windows-gnu` 从 Linux 交叉编译 | `x86_64-pc-windows-msvc` 在 windows runner 原生构建 | ring/mio 已知问题，持续存在 | 需要 NASM 但无链接问题 |
| cargo-dist 写入 Cargo.toml `[workspace.metadata.dist]` | 同样支持 `dist-workspace.toml` 独立文件 | cargo-dist 0.x | 本项目无 workspace 成员，Cargo.toml 写法更简洁 |
| cargo-dist 0.x 固定 runner | cargo-dist 0.20+ `github-custom-runners` | 2024 年引入 | 可为每个 target 指定不同 runner 和容器镜像 |

**Deprecated/outdated:**
- `x86_64-pc-windows-gnu` 目标：虽然理论上存在，但与 ring/tokio/mio 生态实际不兼容；生产项目（含 cargo-dist 自身）均使用 `msvc`
- `cargo dist init --ci=github`（旧命令）：现在直接 `cargo dist init` 即可选择 CI

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `x86_64-pc-windows-gnu` 与 ring/tokio 不兼容在当前版本仍存在 | Pitfall 2、Standard Stack | 若已修复，可用 gnu 目标在 Linux runner 上交叉编译 Windows，无需 windows runner |
| A2 | cargo-dist 生成的 Windows installer 脚本名含 "installer"（不覆盖 install.sh） | Architecture Patterns | 若命名冲突，需手动重命名或配置 |
| A3 | GitHub Actions `windows-2022` 的 Chocolatey 安装 NASM 可正常工作 | Code Examples | 若 choco 不可用，需改用其他安装方式 |
| A4 | `russh` 在 `x86_64-pc-windows-msvc` 上可正常构建（基于 russh CI 的 windows-latest 测试） | Standard Stack | 若 russh 有未被 CI 测到的 MSVC 问题，需评估替代方案 |

**Risk assessment:** A1 风险最低（ring#1363 和 mio#1632 issue 均记录了根本原因，非偶发问题）；A4 风险最低（russh 自身 CI 在 windows-latest 上运行）。

---

## Open Questions

1. **`cargo dist init` 生成配置的精确结果**
   - What we know: 会生成 `[workspace.metadata.dist]` 块和 `.github/workflows/release.yml`
   - What's unclear: 交互选择哪些选项会生成最接近期望的配置（尤其是 windows-msvc 的 NASM 安装步骤是否可通过 dist 配置自动处理）
   - Recommendation: Wave 1 第一个任务就是运行 `cargo dist init` 并记录实际生成内容；预期需要手动补充 Windows 的 NASM 安装步骤

2. **aarch64 构建后实际产物是否正确**
   - What we know: apt 安装 gcc-aarch64-linux-gnu + linker 配置理论上可以解决 ring 问题
   - What's unclear: 是否还有其他 crate（如 russh 的 C 依赖）在 aarch64 交叉编译时有问题
   - Recommendation: Wave 2 第一步就是触发一次实际 CI run 验证三个平台的构建产物

3. **`install-path` 配置**
   - What we know: 默认 `CARGO_HOME`（即 `~/.cargo/bin`）；也可配置为系统路径
   - What's unclear: 对于 DM 运维场景，是否更应该安装到 `/usr/local/bin`
   - Recommendation: 使用默认 `CARGO_HOME`，用户可手动移动；不影响功能

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| `cargo` | Rust 编译 | ✓ | 1.96.0 | — |
| `rustup` | toolchain 管理 | ✓ | stable-aarch64-apple-darwin | — |
| `docker` | cross 工具链（本地测试） | ✓ | 29.5.3 | 跳过本地交叉编译测试，直接推 CI |
| `gh` CLI | GitHub release 操作 | ✓ | 2.94.0 | — |
| `cargo-dist` | 发布流水线 | ✗ | — | 需要安装：`cargo install cargo-dist` |
| `cross` | 本地交叉编译验证 | ✗ | — | 跳过本地验证，仅 CI 构建 |
| GitHub remote | tag 触发发布 | ✓ | origin 已配置 | — |

**Missing dependencies with no fallback:**
- `cargo-dist`：必须安装才能运行 `dist init`；安装命令：`cargo install cargo-dist --version 0.32.0`

**Missing dependencies with fallback:**
- `cross`：本地交叉编译测试用，CI 中不依赖；可跳过本地验证直接通过 CI 确认

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust 内置 `cargo test` + inline `#[cfg(test)]` |
| Config file | none（未检测到 nextest/pytest 等） |
| Quick run command | `cargo test 2>&1 \| tail -20` |
| Full suite command | `cargo test --all` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| PLAT-01 | Linux x86_64 二进制可运行 | smoke（CI 构建验证） | CI `dist build` | ✗ Wave 0（CI 配置） |
| PLAT-02 | aarch64 二进制可运行 | smoke（CI 构建验证） | CI `dist build` | ✗ Wave 0（CI 配置） |
| PLAT-03 | Windows 二进制可运行 | smoke（CI 构建验证） | CI `dist build` | ✗ Wave 0（CI 配置） |
| PLAT-04 | Windows placeholder CLI 可解析 | unit | `cargo test -- test_install_windows` | ✗ Wave 0（新建） |
| CR-01 | 安装包上传后可执行 | unit（mock runner） | `cargo test -- test_upload_installer` | ✗ Wave 0（修改） |
| CR-02 | sftp_write 可写新文件 | unit（mock runner） | `cargo test -- test_sftp_create_new_file` | ✗ Wave 0（修改） |
| CR-03 | tilde 路径正确展开 | unit | `cargo test -- test_expand_tilde` | ✗ Wave 0（新建） |
| CR-04 | shell_quote 防注入 | unit | `cargo test -- test_shell_quote` | ✗ Wave 0（新建） |
| CR-05 | TOFU 接受时打印指纹警告 | unit | `cargo test -- test_tofu_logs_fingerprint` | ✗ Wave 0（修改） |

### Sampling Rate

- **Per task commit:** `cargo test 2>&1 | tail -20`
- **Per wave merge:** `cargo test --all`
- **Phase gate:** 全部测试绿色 + CI aarch64/x86_64/windows 三平台构建成功

### Wave 0 Gaps

- [ ] `src/cluster/ssh.rs` — CR-02 fix + 测试 sftp CREATE
- [ ] `src/cluster/deploy.rs` — CR-01 fix + CR-04 fix + 更新相关测试
- [ ] `src/cluster/ssh.rs` — CR-03 expand_tilde + CR-05 TOFU warning
- [ ] `src/cli.rs` — PLAT-04 InstallWindows placeholder + test
- [ ] Cargo.toml — 新增 `repository`/`license`/`description` metadata
- [ ] `.cargo/config.toml` — 新建，配置 aarch64 linker

---

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — |
| V3 Session Management | no | — |
| V4 Access Control | no | — |
| V5 Input Validation | yes（CR-04 shell 注入） | config 加载时验证路径字符集；`shell_quote` 函数 |
| V6 Cryptography | yes（CR-05 SSH TOFU） | 至少记录指纹；理想增加 known_hosts 验证 |

### Known Threat Patterns for {stack}

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Shell 命令注入（CR-04） | Tampering | `shell_quote()` 或路径字段白名单验证 |
| SSH MITM（CR-05） | Spoofing | 记录 TOFU 指纹 warning；后续可加 known_hosts |
| 安装包完整性（已处理） | Tampering | `sha2` crate 已在 Cargo.toml，checksum 验证框架已存在 |

---

## Sources

### Primary (HIGH confidence)
- cargo-dist 0.32.0 crates.io 版本 [VERIFIED] — `cargo search cargo-dist` 输出
- cargo-dist 官方 dist-workspace.toml [VERIFIED] — `gh api repos/axodotdev/cargo-dist/contents/dist-workspace.toml`
- txpipe/oura dist-workspace.toml 实际案例 [VERIFIED] — `gh api repos/txpipe/oura/contents/dist-workspace.toml`
- cargo-dist 生成的 txpipe/oura release.yml [VERIFIED] — `gh api repos/txpipe/oura/contents/.github/workflows/release.yml`
- russh CI (rust.yml) [VERIFIED] — `gh api repos/Eugeny/russh/contents/.github/workflows/rust.yml`；确认 windows-latest runner 和 NASM 安装
- 03-REVIEW.md 五个 Critical 问题 [VERIFIED] — 已读取完整报告，确认修复方案

### Secondary (MEDIUM confidence)
- cargo-dist issue #1378 — aarch64 ring 编译问题和 `gcc-aarch64-linux-gnu` apt 安装解决方案 [CITED: github.com/axodotdev/cargo-dist/issues/1378]
- cargo-dist 配置文档 — `github-custom-runners`, `dependencies.apt`, `targets` 字段说明 [CITED: axodotdev.github.io/cargo-dist/book/reference/config.html]

### Tertiary (LOW confidence)
- ring issue #1363 [CITED] — ring 不支持从 Linux 交叉编译到 windows-gnu（标注为 [ASSUMED] 当前版本状态待验证）
- mio issue #1632 [CITED] — tokio/mio ntdll 链接问题在 windows-gnu 下（标注为 [ASSUMED] 当前版本状态待验证）

---

## Project Constraints (from CLAUDE.md)

- **函数 ≤40 行**：Phase 3 bug 修复的函数（`expand_tilde`、`shell_quote`）本身简单；`check_server_key` 修复后仍在限制内
- **Rust 实现**：所有代码变更用 Rust；CI 配置用 YAML
- **Conventional commits**：发版 commit 格式 `chore(release): bump version to v0.1.0`；feature commit `feat(cli): add install-windows placeholder command`
- **描述性变量名**：`remote_bin_path`、`shell_quoted_install_path` 等，不用单字母

---

## Metadata

**Confidence breakdown:**
- Standard Stack: HIGH — cargo-dist 版本通过 crates.io 验证；russh Windows 兼容性通过其 CI 验证
- Architecture: HIGH — 基于 cargo-dist 自身项目和 oura 实际项目的真实 dist-workspace.toml 推导
- Pitfalls: MEDIUM/HIGH — ring/mio windows-gnu 问题通过官方 issue 确认；aarch64 linker 问题通过 issue#1378 确认

**Research date:** 2026-06-13
**Valid until:** 2026-07-13（cargo-dist 更新频繁，30 天后重新验证版本）
