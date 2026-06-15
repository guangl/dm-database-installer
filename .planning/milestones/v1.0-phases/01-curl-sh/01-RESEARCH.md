# Phase 1: curl|sh 单机安装 - Research

**Researched:** 2026-06-12
**Domain:** Rust CLI, 达梦数据库静默安装, systemd 服务注册, 本地文件校验
**Confidence:** HIGH (核心技术栈) / MEDIUM (DM 安装细节)

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**安装包获取策略 (DOWN-01 / DOWN-02)**
- D-01: Phase 1 以 `--package /path/to/dm.iso` 本地路径为主交付路径。自动下载通过占位 `download` 模块骨架实现——能跑通流程，下载 URL 待 spike 验证可行性后填入。
- D-02: SHA-256 校验（DOWN-02）作为独立步骤，使用 `sha2` crate；本地路径 + 下载路径都经过校验，不绕过。

**CLI 入口结构**
- D-03: 主命令 `dm-installer install [--package <path>] [--defaults]`；未传 `--package` 时尝试自动下载（占位）。`dm-installer validate --config <file>` 作为独立子命令（QUAL-03）。
- D-04: `--defaults` 跳过所有交互确认（供 `curl | sh` 脚本使用）；后续 Phase 2 的 `--config <toml>` 加在 `install` 子命令上。

**INST-03 不可修改参数确认流程**
- D-05: 默认行为：安装前打印四个不可修改参数的当前值（PAGE_SIZE / CHARSET / CASE_SENSITIVE / EXTENT_SIZE）并等待 `y/n` 用户确认；输入 `n` 则 abort。
- D-06: `--defaults` 或 `--yes` flag 跳过确认，直接继续。`curl | sh` bootstrap 脚本自动传入 `--defaults`，保证管道场景无交互阻塞。

**curl|sh 默认安装参数**
- D-07: 遵循 DM 官方默认值：
  - PAGE_SIZE=8, EXTENT_SIZE=16, CHARSET=GB18030 (0), CASE_SENSITIVE=Y
  - 安装路径：`/opt/dmdbms`
  - 端口：5236
  - 实例名：DMSERVER

**幂等性检测 (QUAL-02)**
- D-08: 安装开始前检测 `/opt/dmdbms/dm.ini` 是否存在。存在则打印提示信息并以 exit code 0 退出，不执行任何安装操作。

### Claude's Discretion

- 日志/进度展示：使用 `indicatif` 进度条（下载）+ `console` 状态消息（安装步骤）；`--verbose` 开启 tracing debug 输出。
- 错误处理：`anyhow` 用于顶层，`thiserror` 用于 download / install 模块的类型化错误。
- 服务注册（INST-04）：Linux 写 systemd unit file 到 `/etc/systemd/system/dmserver.service`，执行 `systemctl enable --now dmserver`；Windows 留占位（Phase 4 处理）。

### Deferred Ideas (OUT OF SCOPE)

- 自动下载 URL (DOWN-01 完整实现) — 需要 spike 验证达梦官网直链可行性；Phase 1 留占位，spike 完成后在 Phase 1/2 间填入
- Windows 服务注册 (INST-04 Windows 分支) — Phase 4 处理
- 断点续传 (DOWN-V2-01) — v2 需求
- `--dry-run` 模式 (OPS-V2-02) — v2 需求
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| INST-01 | 用户可通过 `curl \| sh` 一行命令安装单机达梦数据库，无需提前下载任何文件或编写配置 | clap subcommand + `--defaults` flag 实现无交互流程；`--package` 接受本地 ISO 路径 |
| INST-03 | 安装器在执行 dminit 前，明确展示 PAGE_SIZE/CHARSET/CASE_SENSITIVE/EXTENT_SIZE 四个不可修改参数并要求用户确认 | std::io::stdin + `--defaults` flag 跳过确认；dminit 参数映射已确认 |
| INST-04 | 安装完成后自动将达梦实例注册为系统服务（Linux: systemd），并设置开机自启 | DM 自带 `dm_service_installer.sh` 脚本；注册后 `systemctl enable DmServiceDMSERVER.service` |
| DOWN-01 | 安装器自动从达梦官方渠道下载安装包（Phase 1 为占位骨架） | `reqwest` 异步下载架构；Phase 1 只建模块骨架 |
| DOWN-02 | 下载完成后验证安装包 SHA-256 校验和，校验失败则拒绝继续安装 | `sha2` crate 分块读取文件并计算 SHA-256 |
| QUAL-02 | 安装器检测目标机器上的已有达梦实例，避免重复安装时覆盖或崩溃（幂等性） | 检测 `/opt/dmdbms/dm.ini` 存在性；exit code 0 退出 |
| QUAL-03 | 用户可运行 `dm-installer validate --config config.toml` 仅验证配置文件合法性，不执行实际安装 | clap `validate` subcommand + `toml::from_str` 反序列化验证 |
</phase_requirements>

---

## Summary

Phase 1 实现达梦数据库 (DM8) 的 `curl | sh` 单机安装链路，技术核心是：(1) 接受本地 ISO 路径，通过 `mount -o loop` 或 `bsdtar x` 提取 `DMInstall.bin`，生成 XML 响应文件执行静默安装；(2) 运行 `dminit` 初始化数据库实例；(3) 调用 DM 自带的 `dm_service_installer.sh` 注册 systemd 服务；(4) SHA-256 完整性校验贯穿全程。

技术栈完全在 CLAUDE.md 推荐范围内，所有核心 crate 版本已通过 crates.io API 验证与 CLAUDE.md 一致。关键约束是：安装器全程需要 root 权限（mount ISO、写入 /opt、注册 systemd 服务），因此 INST-01 的 `curl | sh` 流程需要在 bootstrap 脚本中以 `sudo` 或在 root 会话中运行。

**Primary recommendation:** 以 `std::process::Command` 调用外部工具（mount/bsdtar、DMInstall.bin、dminit、dm_service_installer.sh）为核心，通过生成 XML 响应文件驱动 DM 静默安装。Rust 代码负责编排流程、校验、UI 和幂等性检测，不替代 DM 原生安装脚本的功能。

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| CLI 解析与参数验证 | Rust CLI Binary | — | clap derive macro 全部覆盖 |
| ISO 包处理（mount/提取） | OS 系统调用（root） | Rust orchestration | `mount -o loop` 或 `bsdtar x` 需要 root 权限 |
| SHA-256 完整性校验 | Rust (`sha2` crate) | — | 纯 Rust，不依赖外部工具 |
| DM 静默安装 (DMInstall.bin) | DM 原生安装程序 | Rust orchestration | Rust 生成 XML，传给 DMInstall.bin `-q` |
| 数据库初始化 (dminit) | DM 原生工具 | Rust orchestration | Rust 构造命令行参数，std::process::Command 调用 |
| systemd 服务注册 | DM 原生脚本 + systemctl | Rust orchestration | 调用 `dm_service_installer.sh` + `systemctl enable` |
| 用户交互（确认提示） | Rust CLI | — | `std::io::stdin` 读取 y/n；`--defaults` 跳过 |
| 幂等性检测 | Rust (文件系统检测) | — | `Path::exists()` 检测 `/opt/dmdbms/dm.ini` |
| 进度/状态展示 | Rust (`indicatif` + `console`) | — | 安装步骤 spinner + 消息样式 |
| 配置验证 (QUAL-03) | Rust (`toml` + `serde`) | — | `toml::from_str` 反序列化至类型化结构体 |
| 下载（占位） | Rust (`reqwest` 骨架) | — | Phase 1 只建模块接口，不实现真实 URL |

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `clap` | 4.6.1 | CLI argument parsing | [VERIFIED: crates.io] 最高下载量 CLI 库；derive macro 消除样板代码；subcommand 模型直接映射到 `install`/`validate` |
| `tokio` | 1.52.3 | Async runtime | [VERIFIED: crates.io] reqwest/未来 russh 需要；async runtime 统一 |
| `serde` + `serde_derive` | 1.0.228 | 序列化框架 | [VERIFIED: crates.io] TOML 配置反序列化基础 |
| `toml` | 1.1.2 | TOML 配置解析 | [VERIFIED: crates.io] 官方 toml-rs；`toml::from_str` 直接反序列化到 Rust 结构体 |
| `anyhow` | 1.0.102 | 应用级错误处理 | [VERIFIED: crates.io] binary 代码首选；`context()` 链路追踪 |
| `thiserror` | 2.0.18 | 模块级类型化错误 | [VERIFIED: crates.io] `download` / `install` 模块结构化错误；与 anyhow 无缝配合 |
| `sha2` | 0.11.0 | SHA-256 校验和 | [VERIFIED: crates.io] RustCrypto 官方库；DOWN-02 需求 |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `indicatif` | 0.18.4 | 进度条和 spinner | [VERIFIED: crates.io] 下载字节进度 + 安装步骤 spinner |
| `console` | 0.16.3 | 终端颜色和样式输出 | [VERIFIED: crates.io] `[OK]`/`[ERROR]`/`[WARN]` 状态消息；自动 ANSI 检测 |
| `tracing` | 0.1.44 | 结构化 async 日志 | [VERIFIED: crates.io] `--verbose` 映射到 `RUST_LOG` filter |
| `tracing-subscriber` | 0.3.23 | 日志格式化输出 | [VERIFIED: crates.io] EnvFilter + fmt layer |
| `tempfile` | 3.27.0 | 安全临时目录管理 | [VERIFIED: crates.io] XML 响应文件、ISO 提取目录；drop 时自动清理 |
| `reqwest` | 0.13.4 | HTTP 下载（占位骨架） | [VERIFIED: crates.io] Phase 1 只建 download 模块接口；使用 `rustls-tls` feature，禁止 `native-tls` |
| `clap_complete` | 4.6.5 | Shell 补全生成 | [VERIFIED: crates.io] `dm-installer completions bash/zsh/fish` 子命令 |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `std::process::Command` 调外部工具 | 纯 Rust ISO9660 crate (cdfs/iso9660) | ISO crate 下载量极低 (<8k)，缺乏生产验证；而 `mount`/`bsdtar` 在目标 Linux 系统普遍存在且经过验证 |
| DM 自带 `dm_service_installer.sh` | 手写 systemd unit file | DM 的服务脚本包含 DM 特定的启动/停止/状态逻辑，手写会遗漏；官方脚本是正确选择 |
| `std::io::stdin` 读取 y/n | `dialoguer` crate | dialoguer 增加依赖；Phase 1 的简单 y/n 确认不需要 TUI 库 |

**Installation:**
```toml
[package]
name = "dm-database-installer"
version = "0.1.0"
edition = "2024"

[dependencies]
clap = { version = "4.6.1", features = ["derive"] }
tokio = { version = "1.52.3", features = ["full"] }
serde = { version = "1.0.228", features = ["derive"] }
toml = "1.1.2"
anyhow = "1.0.102"
thiserror = "2.0.18"
sha2 = "0.11.0"
indicatif = "0.18.4"
console = "0.16.3"
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
tempfile = "3.27.0"
reqwest = { version = "0.13.4", features = ["rustls-tls", "stream"], default-features = false }
clap_complete = "4.6.5"
```

---

## Package Legitimacy Audit

> slopcheck 在此环境不可用，所有包标记 `[ASSUMED]`，但已通过 crates.io API + 官方 GitHub 仓库交叉验证。

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `clap` | crates.io | ~8 yrs | 890M | github.com/clap-rs/clap | N/A | Approved [ASSUMED] |
| `tokio` | crates.io | ~7 yrs | 731M | github.com/tokio-rs/tokio | N/A | Approved [ASSUMED] |
| `serde` | crates.io | ~9 yrs | 1072M | github.com/serde-rs/serde | N/A | Approved [ASSUMED] |
| `toml` | crates.io | ~9 yrs | 679M | github.com/toml-rs/toml | N/A | Approved [ASSUMED] |
| `reqwest` | crates.io | ~7 yrs | 524M | github.com/seanmonstar/reqwest | N/A | Approved [ASSUMED] |
| `anyhow` | crates.io | ~5 yrs | 734M | github.com/dtolnay/anyhow | N/A | Approved [ASSUMED] |
| `thiserror` | crates.io | ~5 yrs | 1081M | github.com/dtolnay/thiserror | N/A | Approved [ASSUMED] |
| `sha2` | crates.io | ~8 yrs | 677M | github.com/RustCrypto/hashes | N/A | Approved [ASSUMED] |
| `indicatif` | crates.io | ~7 yrs | 166M | github.com/console-rs/indicatif | N/A | Approved [ASSUMED] |
| `console` | crates.io | ~7 yrs | 262M | github.com/console-rs/console | N/A | Approved [ASSUMED] |
| `tracing` | crates.io | ~5 yrs | 647M | github.com/tokio-rs/tracing | N/A | Approved [ASSUMED] |
| `tracing-subscriber` | crates.io | ~5 yrs | 455M | github.com/tokio-rs/tracing | N/A | Approved [ASSUMED] |
| `tempfile` | crates.io | ~8 yrs | 618M | github.com/Stebalien/tempfile | N/A | Approved [ASSUMED] |
| `clap_complete` | crates.io | ~5 yrs | 83M | github.com/clap-rs/clap | N/A | Approved [ASSUMED] |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

*slopcheck 在此环境不可用；所有包通过 crates.io API 验证下载量、创建时间和官方 GitHub 仓库，均为高度可信的 Rust 生态基础库。*

---

## Architecture Patterns

### System Architecture Diagram

```
用户 / curl|sh
       │
       ▼
 dm-installer install [--package <iso>] [--defaults]
       │
       ├─── 幂等性检测 ──────────► /opt/dmdbms/dm.ini 已存在? ──► exit 0 (提示已安装)
       │
       ├─── 包获取 ─────────────► --package 路径存在?
       │                              ├── YES: 使用本地 ISO
       │                              └── NO:  download::fetch() (占位,未来实现)
       │
       ├─── SHA-256 校验 ────────► sha2::Sha256 分块读取 → 比对预期值
       │                              └── 失败 → abort (exit 1)
       │
       ├─── ISO 解压 ───────────► bsdtar x -f dm.iso -C <tempdir> DMInstall.bin
       │                         (备选: mount -o loop; 需确认目标环境有 bsdtar)
       │
       ├─── 参数确认 (INST-03) ──► 打印 4 个不可修改参数
       │                              ├── --defaults: 跳过
       │                              └── 交互: 等待 y/n
       │
       ├─── 生成 XML 响应文件 ──► tempfile::NamedTempFile → install.xml
       │
       ├─── DMInstall.bin -q ───► std::process::Command::new(dminstall_bin)
       │                              .arg("-q").arg(xml_path)
       │                              .status()
       │
       ├─── dminit ─────────────► std::process::Command::new(dminit_path)
       │                              .arg(format!("PATH={}", data_path))
       │                              .arg(format!("PAGE_SIZE={}", page_size))
       │                              ... (其他参数)
       │                              .status()
       │
       ├─── 服务注册 (INST-04) ─► dm_service_installer.sh -t dmserver
       │                              -dm_ini /opt/dmdbms/data/DAMENG/dm.ini
       │                              -p DMSERVER
       │                          + systemctl enable --now DmServiceDMSERVER.service
       │
       └─── 完成 ───────────────► 打印成功消息 + 连接信息

dm-installer validate --config <toml>
       │
       └─── toml::from_str::<InstallConfig>() ──► Ok: 打印 "配置合法"
                                                    Err: 打印错误详情, exit 1
```

### Recommended Project Structure

```
src/
├── main.rs              # clap CLI 入口，dispatch 到子命令
├── cli.rs               # clap 结构体定义 (Cli, Commands, InstallArgs, ValidateArgs)
├── install/
│   ├── mod.rs           # pub fn install(args: &InstallArgs) -> Result<()>
│   ├── idempotent.rs    # 幂等性检测: check_existing_instance()
│   ├── package.rs       # ISO 处理: extract_dminstall_bin()
│   ├── checksum.rs      # SHA-256 校验: verify_sha256()
│   ├── silent_install.rs # XML 生成 + DMInstall.bin -q 调用
│   ├── init.rs          # dminit 调用封装
│   └── service.rs       # systemd 服务注册
├── download/
│   └── mod.rs           # 占位骨架: pub fn fetch(url: &str) -> Result<PathBuf>
├── config/
│   ├── mod.rs           # InstallConfig 结构体 (Phase 2 扩展点)
│   └── validate.rs      # validate 子命令实现
└── ui.rs                # indicatif progress bar + console 样式消息封装
```

### Pattern 1: clap Derive Macro with Subcommands

**What:** 使用 `#[derive(Parser)]` 和 `#[derive(Subcommand)]` 定义 CLI 结构
**When to use:** 所有 CLI 入口点

```rust
// Source: https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "dm-installer", version, about = "达梦数据库安装器")]
pub struct Cli {
    /// 启用 verbose 日志输出
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 安装达梦数据库单机实例
    Install(InstallArgs),
    /// 验证 TOML 配置文件合法性（不执行安装）
    Validate(ValidateArgs),
    /// 生成 shell 补全脚本
    Completions { shell: clap_complete::Shell },
}

#[derive(clap::Args)]
pub struct InstallArgs {
    /// 本地 ISO 安装包路径
    #[arg(long)]
    pub package: Option<std::path::PathBuf>,

    /// 跳过所有交互确认（curl | sh 模式使用）
    #[arg(long)]
    pub defaults: bool,

    /// 等同于 --defaults
    #[arg(long, short = 'y')]
    pub yes: bool,
}
```

### Pattern 2: DM 静默安装 XML 响应文件生成

**What:** 动态生成 XML 驱动 `DMInstall.bin -q`
**When to use:** 执行 DM 安装前

```rust
// Source: 基于 cloud.tencent.com/developer/article/2373070 + blog.csdn.net/qq_37822702/article/details/135692094
// [CITED: cloud.tencent.com/developer/article/2373070]
use tempfile::NamedTempFile;
use std::io::Write;

pub fn generate_install_xml(
    install_path: &str,
    data_path: &str,
    instance_name: &str,
    port: u16,
    page_size: u8,     // 4/8/16/32
    charset: u8,       // 0=GB18030, 1=UTF-8, 2=EUC-KR
    case_sensitive: bool,
    extent_size: u8,   // 16/32
    sysdba_pwd: &str,
) -> anyhow::Result<NamedTempFile> {
    let xml = format!(r#"<?xml version="1.0"?>
<DATABASE>
  <LANGUAGE>zh</LANGUAGE>
  <TIME_ZONE>+08:00</TIME_ZONE>
  <INSTALL_TYPE>0</INSTALL_TYPE>
  <INSTALL_PATH>{install_path}</INSTALL_PATH>
  <INIT_DB>Y</INIT_DB>
  <DB_PARAMS>
    <PATH>{data_path}</PATH>
    <DB_NAME>DAMENG</DB_NAME>
    <INSTANCE_NAME>{instance_name}</INSTANCE_NAME>
    <PORT_NUM>{port}</PORT_NUM>
    <PAGE_SIZE>{page_size}</PAGE_SIZE>
    <CHARSET>{charset}</CHARSET>
    <CASE_SENSITIVE>{case}</CASE_SENSITIVE>
    <EXTENT_SIZE>{extent_size}</EXTENT_SIZE>
    <SYSDBA_PWD>{sysdba_pwd}</SYSDBA_PWD>
    <CREATE_DB_SERVICE>N</CREATE_DB_SERVICE>
    <STARTUP_DB_SERVICE>N</STARTUP_DB_SERVICE>
  </DB_PARAMS>
</DATABASE>"#,
        case = if case_sensitive { "Y" } else { "N" },
    );
    let mut file = NamedTempFile::new()?;
    file.write_all(xml.as_bytes())?;
    Ok(file)
}
```

> **重要发现：** XML 中 `<CREATE_DB_SERVICE>` 和 `<STARTUP_DB_SERVICE>` 建议设为 `N`，由 Rust 代码在安装后通过 `dm_service_installer.sh` 单独注册服务，这样 Rust 可以精确控制服务名称和验证结果。

### Pattern 3: SHA-256 文件校验

**What:** 分块读取大文件并计算 SHA-256
**When to use:** 包获取后立即执行（DOWN-02）

```rust
// Source: https://docs.rs/sha2/latest/sha2/ [CITED]
use sha2::{Sha256, Digest};
use std::{fs::File, io::{BufReader, Read}};

pub fn verify_sha256(path: &std::path::Path, expected_hex: &str) -> anyhow::Result<()> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536]; // 64KB chunks
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    let result = format!("{:x}", hasher.finalize());
    if result != expected_hex {
        anyhow::bail!(
            "SHA-256 校验失败\n  期望: {}\n  实际: {}", expected_hex, result
        );
    }
    Ok(())
}
```

### Pattern 4: systemd 服务注册

**What:** 调用 DM 自带脚本注册并启用服务
**When to use:** dminit 完成后（INST-04）

```rust
// Source: MEDIUM confidence, 基于 cnblogs.com/Williamls/p/17088354.html [CITED]
use std::process::Command;

pub fn register_systemd_service(
    dm_home: &str,          // e.g., "/opt/dmdbms"
    dm_ini_path: &str,      // e.g., "/opt/dmdbms/data/DAMENG/dm.ini"
    service_name: &str,     // e.g., "DMSERVER"
) -> anyhow::Result<()> {
    let installer_script = format!("{}/script/root/dm_service_installer.sh", dm_home);

    // Step 1: 使用 DM 自带脚本注册服务（结果: DmServiceDMSERVER.service）
    let status = Command::new("bash")
        .arg(&installer_script)
        .arg("-t").arg("dmserver")
        .arg("-dm_ini").arg(dm_ini_path)
        .arg("-p").arg(service_name)
        .status()
        .with_context(|| "执行 dm_service_installer.sh 失败")?;
    anyhow::ensure!(status.success(), "dm_service_installer.sh 返回非零退出码");

    // Step 2: 启用并立即启动服务
    let svc = format!("DmService{}.service", service_name);
    let status = Command::new("systemctl")
        .arg("enable").arg("--now").arg(&svc)
        .status()
        .with_context(|| format!("systemctl enable --now {} 失败", svc))?;
    anyhow::ensure!(status.success(), "systemctl enable --now 返回非零退出码");
    Ok(())
}
```

### Pattern 5: ISO 包提取（关键决策点）

**What:** 从 DM8 ISO 中提取 `DMInstall.bin`
**When to use:** 收到本地 ISO 路径后

```rust
// [ASSUMED] - 需在目标 Linux 环境验证 bsdtar 可用性
// 两种策略，按优先级尝试：

// 策略 A: 使用 bsdtar (无需 root，大多数 Linux 发行版预装)
// bsdtar x -f /path/to/dm.iso -C /tmp/dm_extract
let status = Command::new("bsdtar")
    .args(["x", "-f", iso_path, "-C", extract_dir])
    .status()?;

// 策略 B: mount -o loop (需要 root，installer 本身已以 root 运行)
// mount -o loop /path/to/dm.iso /mnt/dm_iso
// cp /mnt/dm_iso/DMInstall.bin /tmp/dm_extract/
// umount /mnt/dm_iso
```

> **注意（见 Pitfall 3）：** `bsdtar` 在目标 Linux 发行版（CentOS/RHEL/Kylin）的可用性需要在实际测试环境中验证。安全起见，实现中应检测 bsdtar 是否存在，不存在则 fallback 到 mount 策略。

### Pattern 6: 交互确认（INST-03）

**What:** 展示不可修改参数并等待用户 y/n 确认
**When to use:** 执行 dminit 前；`--defaults` 跳过

```rust
use std::io::{self, Write, BufRead};
use console::{style, Term};

pub fn confirm_immutable_params(
    page_size: u8,
    charset: &str,
    case_sensitive: bool,
    extent_size: u8,
    skip: bool,  // --defaults 或 --yes
) -> anyhow::Result<()> {
    let term = Term::stdout();
    term.write_line(&format!("{}", style("以下参数安装后不可修改：").yellow().bold()))?;
    term.write_line(&format!("   PAGE_SIZE        : {}", page_size))?;
    term.write_line(&format!("   CHARSET          : {}", charset))?;
    term.write_line(&format!("   CASE_SENSITIVE   : {}", if case_sensitive { "Y" } else { "N" }))?;
    term.write_line(&format!("   EXTENT_SIZE      : {}", extent_size))?;

    if skip {
        term.write_line("确认继续安装？[y/N] y (--defaults 自动确认)")?;
        return Ok(());
    }

    print!("确认继续安装？[y/N] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    if input.trim().to_lowercase() != "y" {
        anyhow::bail!("用户取消安装");
    }
    Ok(())
}
```

### Anti-Patterns to Avoid

- **直接调用 tokio 异步 stdin 读取 y/n 确认：** tokio 的 stdin 实现在底层使用独立线程进行阻塞读取，无法取消。用 `std::io::stdin()` 同步读取更简单可靠（见 docs.rs/tokio/tokio::io::Stdin 说明）[CITED]。
- **在 XML 中将 `CREATE_DB_SERVICE=Y`：** DM 的自动服务注册不可控，Rust 代码无法验证注册结果；改为手动调用 `dm_service_installer.sh`。
- **`reqwest` 使用 `native-tls` feature：** 导致 OpenSSL 依赖，破坏跨编译；必须用 `rustls-tls` + `default-features = false`（CLAUDE.md 明确禁止）。
- **在 `--defaults` 模式下读取 stdin：** `curl | sh` 管道会立即关闭 stdin，读取会返回 EOF 导致错误；必须在传入 `--defaults` 时完全跳过 stdin 读取。

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SHA-256 校验 | 自写哈希循环 | `sha2` crate (RustCrypto) | 密码学实现有已知陷阱；`sha2` 经过审计 |
| 临时目录管理 | `std::fs::create_dir_temp` + 手动清理 | `tempfile::TempDir` | drop 时自动清理；panic 安全；跨平台 |
| systemd 服务 unit 文件 | 手写 `[Unit]/[Service]/[Install]` | DM 自带 `dm_service_installer.sh` | DM 服务有特定的启停顺序和信号处理；官方脚本正确处理这些细节 |
| 进度条 | `eprintln!` 打点 | `indicatif` | TTY 检测、字节格式化、速度估算、ETA 已内置 |
| 终端颜色 | ANSI escape codes 硬编码 | `console` | 自动检测 non-TTY/CI 场景，回退到纯文本 |
| XML 解析/生成 | 手写字符串拼接（生产代码） | Rust format! 宏 + NamedTempFile | XML 简单固定结构时 format! 可接受；但注意路径中特殊字符转义 |

**Key insight:** DM 安装流程最复杂的部分（二进制安装器、dminit 参数计算、服务脚本）都已由达梦提供。Rust 代码的价值在于编排这些工具的调用顺序、处理错误、提供良好 UX——而不是替代它们。

---

## Common Pitfalls

### Pitfall 1: ISO 挂载需要 root，安装器本身也需要 root
**What goes wrong:** 用户以普通用户运行安装器，mount 失败，用户不明白原因。
**Why it happens:** `mount -o loop` 是特权操作；DM 安装到 `/opt` 也需要写权限；`dm_service_installer.sh` 写 `/etc/systemd/system/` 需要 root。
**How to avoid:** 安装器启动时立即检测 root 身份。Phase 1 的 PROJECT.md 约束「无 C FFI 依赖」，因此不引入 `libc` crate；改用纯 std 方案：读取 `/proc/self/status` 中 `Uid:` 行的第一个字段是否为 `0`，fallback 检查 `std::env::var("USER") == "root"`。不满足则打印明确提示并以非零退出码退出（或自动 `sudo` 重新执行，参考 rustup 模式）。
**Warning signs:** `mount: only root can use "--options" option` 错误信息。

### Pitfall 2: dminit 等号两侧不能有空格
**What goes wrong:** `dminit PATH = /data` 无效，参数被忽略或报错。
**Why it happens:** dminit 使用简单字符串解析，不兼容 `key = value` 格式。
**How to avoid:** 所有参数用 `format!("KEY={}", value)` 不含空格，通过 `.arg()` 单独传递每个参数。
**Warning signs:** dminit 用默认值初始化而非指定值。

### Pitfall 3: bsdtar 在部分 Linux 发行版不预装
**What goes wrong:** `bsdtar x -f dm.iso` 在 CentOS/RHEL minimal 安装上失败 (`command not found`)。
**Why it happens:** bsdtar (libarchive) 在 Ubuntu 默认安装，但 RHEL/CentOS minimal 可能不包含。
**How to avoid:** 检测 `bsdtar` 是否可用；若不可用，fallback 到 `mount -o loop`；若 mount 也不可用，打印明确安装建议。**在实际目标发行版上测试此流程。**
**Warning signs:** `command not found: bsdtar`。

### Pitfall 4: `--defaults` 模式下 stdin 已关闭
**What goes wrong:** `curl | sh` 管道中，sh 脚本的 stdin 与 curl 输出绑定，安装器 stdin 可能为空或 EOF；任何 `stdin().read_line()` 调用立即返回空字符串，导致 "用户取消" 或死循环。
**Why it happens:** shell 管道中子进程继承 shell 的 stdin，而 `curl | sh` 中 shell 的 stdin 是 curl 的输出流。
**How to avoid:** `--defaults` 或 `--yes` flag 时，完全不执行任何 stdin 读取。bootstrap 脚本调用时必须传 `--defaults`。
**Warning signs:** 非交互场景下安装卡住或立即退出。

### Pitfall 5: 达梦安装路径权限问题（dmdba 用户）
**What goes wrong:** DM 推荐安装时创建 `dmdba` 用户并将安装目录归该用户所有；如果以 root 运行 dminit，后续服务以 dmdba 用户运行时可能遇到文件权限问题。
**Why it happens:** DM 的标准安装流程通常包含 `useradd dmdba` 步骤，但静默安装 XML 中未必自动创建。
**How to avoid:** 在 XML 或安装后步骤中验证 dmdba 用户已创建或由安装脚本创建；如果在 CI/容器环境跳过了 `root_installer.sh`，需要手动确认权限。
**Warning signs:** 服务启动失败，日志显示权限拒绝。

### Pitfall 6: 幂等性检测路径硬编码与配置路径不一致
**What goes wrong:** 幂等性检测硬编码检查 `/opt/dmdbms/dm.ini`，但用户通过 `--package` + 自定义路径安装时，实际 dm.ini 在不同位置。
**Why it happens:** Phase 1 默认路径是 `/opt/dmdbms`，但代码结构需要为 Phase 2 参数化路径做准备。
**How to avoid:** 将安装路径作为 `InstallConfig` 结构体的字段（即使 Phase 1 硬编码默认值），幂等性检测使用配置中的路径而非常量字符串。

---

## Code Examples

### 完整 dminit 调用

```rust
// Source: [ASSUMED] 基于 eco.dameng.com/document/dm/zh-cn/ops/installation-install [CITED]
use std::process::Command;

pub fn run_dminit(config: &DminitConfig) -> anyhow::Result<()> {
    let dminit_bin = format!("{}/bin/dminit", config.dm_home);
    let status = Command::new(&dminit_bin)
        .arg(format!("PATH={}", config.data_path))
        .arg(format!("DB_NAME={}", config.db_name))
        .arg(format!("INSTANCE_NAME={}", config.instance_name))
        .arg(format!("PORT_NUM={}", config.port))
        .arg(format!("PAGE_SIZE={}", config.page_size))
        .arg(format!("EXTENT_SIZE={}", config.extent_size))
        .arg(format!("CHARSET={}", config.charset))
        .arg(format!("CASE_SENSITIVE={}", if config.case_sensitive { "Y" } else { "N" }))
        .arg(format!("SYSDBA_PWD={}", config.sysdba_pwd))
        .arg(format!("SYSAUDITOR_PWD={}", config.sysauditor_pwd))
        .status()
        .with_context(|| format!("执行 dminit 失败: {}", dminit_bin))?;
    anyhow::ensure!(status.success(), "dminit 返回非零退出码: {:?}", status.code());
    Ok(())
}
```

### DMInstall.bin 静默安装调用

```rust
// Source: [CITED: cloud.tencent.com/developer/article/2373070]
pub fn run_silent_install(dminstall_bin: &str, xml_path: &str) -> anyhow::Result<()> {
    let status = Command::new(dminstall_bin)
        .arg("-q")
        .arg(xml_path)  // 必须是绝对路径
        .status()
        .with_context(|| "DMInstall.bin -q 执行失败")?;
    anyhow::ensure!(status.success(), "DMInstall.bin 返回非零退出码");
    Ok(())
}
```

### reqwest 下载骨架（Phase 1 占位）

```rust
// Source: [ASSUMED] Phase 1 占位骨架，URL 待 spike 验证后填入
use reqwest::Client;

pub async fn fetch_dm_installer(url: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    // TODO: Phase 1 占位 — URL 需要 spike 验证达梦官网直链可行性
    // 实际实现模式参考：
    // let client = Client::new();
    // let resp = client.get(url).send().await?;
    // let total = resp.content_length().unwrap_or(0);
    // let pb = indicatif::ProgressBar::new(total);
    // ...stream chunks, write to dest, pb.inc(bytes)...
    Err(anyhow::anyhow!(
        "自动下载未实现（Phase 1）。请使用 --package /path/to/dm.iso 指定本地安装包"
    ))
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `structopt` derive macro | `clap` v4 derive macro（structopt 已归档） | 2022 | structopt 不再维护，clap 4 内置 derive |
| `reqwest` + `native-tls` (OpenSSL) | `reqwest` + `rustls-tls` (pure Rust) | 2020+ | 消除跨编译时 OpenSSL 依赖地狱 |
| `log` + `env_logger` | `tracing` + `tracing-subscriber` | 2021+ | async span 支持；tokio 生态更一致 |
| `ssh2` (libssh2 C FFI) | `russh` (pure Rust async) | 2022+ | 无 C FFI，跨编译友好（Phase 3 适用） |

**Deprecated/outdated:**
- `structopt`：已归档，用 clap 4 derive 替代
- `reqwest` 使用 `native-tls` feature：导致 OpenSSL 依赖，CLAUDE.md 明确禁止

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `bsdtar` 在目标 Linux 发行版（RHEL/CentOS/Kylin）预装 | Architecture Patterns §Pattern 5 | 需 fallback 到 mount，需额外测试 |
| A2 | DM8 ISO 包含直接可执行的 `DMInstall.bin`（无需额外依赖） | Architecture Patterns §Pattern 5 | 若 DMInstall.bin 有动态库依赖需处理 |
| A3 | `dm_service_installer.sh` 脚本路径为 `{dm_home}/script/root/dm_service_installer.sh` | Pattern 4 | 不同 DM 版本路径可能变化；需运行时验证 |
| A4 | 达梦 dminit 在 root 用户下直接调用可正常工作（不强依赖 dmdba 用户） | Pattern §dminit | 部分版本可能要求 dmdba 用户 |
| A5 | SHA-256 校验时期望值来源（暂无官方 checksum 文件） | Standard Stack | DOWN-02 实现时需确认 checksum 来源（官方页面、manifest 文件、或用户手动提供） |
| A6 | 达梦 ISO 安装包内 `DMInstall.bin` 位于根目录（不在子目录） | Package handling | 若在子目录，bsdtar 提取路径需调整 |

---

## Open Questions (RESOLVED)

> 全部 3 个 open questions 已在 Phase 1 计划阶段解决。下方记录每个问题的 RESOLVED 决策，作为后续实现的权威依据。

1. **达梦 ISO 包的 SHA-256 校验和来源**
   - What we know: DOWN-02 要求校验，但达梦官网未在搜索结果中找到公开的 checksum 文件
   - What's unclear: 官方是否提供 `.sha256` 文件、manifest，还是需要用户手动提供期望值
   - **RESOLVED:** Phase 1 使用可选的 `--checksum <sha256>` 参数；用户提供则严格校验，未提供则 `tracing::warn!` 跳过并继续（Plan 02 Task 2 已实现）。后续 spike 完成 DOWN-01 自动下载时再确定官方 checksum 来源（manifest 文件 / `.sha256` 同伴文件 / 官网 HTML 解析）。

2. **bsdtar vs mount -o loop 策略选择**
   - What we know: bsdtar 无需 root 提取 ISO；mount 需要 root 但安装器本身已需要 root
   - What's unclear: 目标部署环境（RHEL 8/9、CentOS 7、银河麒麟）是否预装 bsdtar
   - **RESOLVED:** 运行时检测 bsdtar 可用性（`Command::new("bsdtar").arg("--version").output()`）；优先使用 bsdtar，不可用则 fallback 到 `mount -o loop /path/to/dm.iso /mnt/dm_iso`；两者均不可用时打印明确错误并提示安装命令（`yum install bsdtar` 或 `apt install libarchive-tools`）。该策略在 Plan 03 Task 1 (ISO 提取) 落地。

3. **dmdba 用户处理策略**
   - What we know: 标准 DM 安装流程创建 dmdba 用户；静默安装 XML 的 `CREATE_DB_SERVICE=N` 跳过了服务脚本
   - What's unclear: `dm_service_installer.sh` 是否要求 dmdba 用户已存在
   - **RESOLVED:** 在调用 `dm_service_installer.sh` 前用 `id dmdba` 检测用户存在性；不存在则 `tracing::warn!` 提示但继续（DM 自带脚本通常会在内部创建用户）；不主动 `useradd`，避免在容器/CI 环境产生意外副作用。该策略在 Plan 04 Task 1 (服务注册) 落地。

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | 编译 | ✓ | 1.96.0 | — |
| Docker | `cross` 跨编译 | ✓ | 29.5.3 (colima) | 手动安装 cross target |
| `cross` CLI | 跨编译到 Linux x86/ARM | ✗ | — | `rustup target add` + 手动配置 linker |
| `cargo-nextest` | 快速测试运行 | ✗ | — | `cargo test` |
| `bsdtar` (目标 Linux) | ISO 提取 | [ASSUMED] 取决于发行版 | — | `mount -o loop` |
| `systemctl` (目标 Linux) | 服务注册验证 | ✓ (Linux 目标) | — | — |
| `dminit` (DM 安装后) | 数据库初始化 | 安装后可用 | — | — |

**Missing dependencies with no fallback:**
- DM8 ISO 安装包本身（本地 `--package` 路径）— 用户必须提供

**Missing dependencies with fallback:**
- `cross`: 没有 cross 仍可通过 `rustup target add x86_64-unknown-linux-gnu` 直接编译（在 macOS ARM 开发机上需要 cross）
- `bsdtar`: fallback 到 `mount -o loop`

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` (built-in) + `cargo-nextest` (推荐安装) |
| Config file | none — Wave 0 无需独立配置 |
| Quick run command | `cargo test` |
| Full suite command | `cargo test -- --include-ignored` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| INST-01 | CLI 解析 `install --package <path> --defaults` 正确 | unit | `cargo test cli::tests::test_install_args` | ❌ Wave 0 |
| INST-03 | `--defaults` 跳过确认；无 flag 时等待 y/n | unit | `cargo test install::tests::test_confirm_params` | ❌ Wave 0 |
| INST-04 | 生成正确的 dm_service_installer.sh 调用命令 | unit | `cargo test install::service::tests::test_service_cmd` | ❌ Wave 0 |
| DOWN-02 | SHA-256 校验：正确 hash 通过，错误 hash 失败 | unit | `cargo test install::checksum::tests::test_sha256` | ❌ Wave 0 |
| QUAL-02 | 检测到 `/opt/dmdbms/dm.ini` 存在时返回幂等跳过 | unit | `cargo test install::idempotent::tests::test_existing_instance` | ❌ Wave 0 |
| QUAL-03 | `validate` 子命令：合法 TOML 返回 Ok，非法 TOML 返回 Err | unit | `cargo test config::validate::tests::test_validate_config` | ❌ Wave 0 |
| INST-01 (e2e) | 完整安装流程在真实 Linux 环境跑通 | manual-only | — | N/A — 需要真实 DM ISO |

### Sampling Rate

- **Per task commit:** `cargo test`
- **Per wave merge:** `cargo test -- --include-ignored`
- **Phase gate:** 所有 unit tests 绿灯 + 手动 e2e 验证（真实 Linux + DM ISO）

### Wave 0 Gaps

- [ ] `src/cli.rs` — CLI 结构体定义，覆盖 INST-01
- [ ] `src/install/checksum.rs` + `tests/` — SHA-256 单元测试，覆盖 DOWN-02
- [ ] `src/install/idempotent.rs` + `tests/` — 幂等性检测，覆盖 QUAL-02
- [ ] `src/config/validate.rs` + `tests/fixtures/valid.toml` + `tests/fixtures/invalid.toml` — validate 子命令，覆盖 QUAL-03
- [ ] `src/install/service.rs` + `tests/` — 服务注册命令构建，覆盖 INST-04

---

## Security Domain

> security_enforcement 未显式配置为 false，视为启用。

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — (CLI 工具，无用户认证) |
| V3 Session Management | no | — |
| V4 Access Control | yes | root 权限检测；安装器本身需 root |
| V5 Input Validation | yes | clap 验证 `--package` 路径；`toml::from_str` 类型化验证配置 |
| V6 Cryptography | yes | `sha2` (RustCrypto) — 不手写哈希；安装包完整性验证 |

### Known Threat Patterns for CLI Install Tools

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| 路径遍历 (`--package ../../etc/passwd`) | Tampering | `Path::canonicalize()` + 检查路径在期望目录内 |
| 恶意 ISO 包（TOCTOU：校验后替换） | Tampering | 校验后立即提取，不在校验和执行之间有延迟 |
| 安装路径注入（XML 中特殊字符） | Tampering | XML 生成时对路径进行 XML 字符转义（`&` → `&amp;` 等） |
| 安装后执行任意代码（DMInstall.bin 来自不可信源） | Elevation of Privilege | SHA-256 校验（DOWN-02）阻断篡改包 |

---

## Sources

### Primary (HIGH confidence)
- `crates.io` API (verified 2026-06-12 via curl) — 所有 crate 版本号和下载量
- [clap docs.rs derive tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) — subcommand 模式
- [sha2 docs.rs](https://docs.rs/sha2/latest/sha2/) — Digest trait 使用
- [tokio process docs](https://docs.rs/tokio/latest/tokio/process/) — stdin 交互注意事项
- [eco.dameng.com: 单机安装部署](https://eco.dameng.com/document/dm/zh-cn/ops/installation-install) — dminit 参数，ISO 挂载流程
- [eco.dameng.com: 安装 FAQ](https://eco.dameng.com/document/dm/zh-cn/faq/faq-dm-install.html) — 不可修改参数说明
- CLAUDE.md §Technology Stack — 推荐库版本（项目权威来源）

### Secondary (MEDIUM confidence)
- [cloud.tencent.com: DM8 静默安装](https://cloud.tencent.com/developer/article/2373070) — XML 响应文件完整格式
- [CSDN: 达梦数据库静默安装](https://blog.csdn.net/qq_37822702/article/details/135692094) — XML 参数对照
- [cnblogs: DM8 安装文档](https://www.cnblogs.com/Williamls/p/17088354.html) — dm_service_installer.sh 命令和服务注册流程
- [CSDN: 达梦开机自启](https://blog.csdn.net/limintjhn8820/article/details/141037213) — systemd 服务结构

### Tertiary (LOW confidence)
- WebSearch: bsdtar ISO 提取（需在目标发行版实测）

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — 所有 crate 版本通过 crates.io API 实时验证，与 CLAUDE.md 完全一致
- Architecture: HIGH — clap/sha2/process::Command 模式来自官方文档；DM 安装流程来自多个社区文档交叉验证
- DM 安装细节: MEDIUM — 基于社区文档，无法直接访问官方 PDF 手册；核心 XML 格式有两个独立来源验证
- Pitfalls: MEDIUM — ISO 提取工具可用性和 dmdba 用户需求需在实际目标环境测试

**Research date:** 2026-06-12
**Valid until:** 2026-07-12 (30 天；达梦文档变化较慢)
