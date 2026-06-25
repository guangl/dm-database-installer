<!-- GSD:project-start source:PROJECT.md -->
## Project

**达梦数据库安装器 (dm-database-installer)**

两层架构：
- **Phase 1 — `install.sh`（纯 shell 脚本）**: `curl | sh` 单机静默安装，面向开发者快速拉起环境，无需编译
- **Phase 2+ — `dm-installer`（Rust 二进制）**: TOML 配置文件驱动的精细安装，面向 DBA/运维，支持自定义参数、主备集群（SSH 远程）、DSC/DPC 集群

**Core Value:** 开发者一行命令搞定本地达梦环境，DBA 用配置文件完成生产集群部署——两类用户都不需要手动操作达梦原生安装程序。

### Constraints

- **Phase 1 实现**: 纯 bash/sh 脚本（`install.sh`）— 无外部依赖，curl|sh 友好
- **Phase 2+ 实现**: Rust — 性能和跨平台部署需求
- **Config Format**: TOML — Rust 生态首选，层级嵌套自然（Phase 2+）
- **Version Strategy**: 固定单版本 — 官网最新，无需版本矩阵
- **Distribution (单行命令)**: `curl | sh` 风格 — 开发者最低摩擦体验
- **Cluster Execution**: 单点 SSH 远程推送 — 用户无需在每个节点手动操作（Phase 3+）
- **Platforms**: Linux (x86/ARM) 主要；Windows 通过 Rust 二进制支持（Phase 2+）
<!-- GSD:project-end -->

<!-- GSD:stack-start source:research/STACK.md -->
## Technology Stack

## Recommended Stack
### Core Technologies
| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| `clap` | 4.6.1 | CLI argument parsing | De facto standard; derive macro eliminates boilerplate; built-in shell completion generation via `clap_complete`; `#[command(subcommand)]` maps cleanly onto `install standalone`, `install cluster` subcommand model |
| `tokio` | 1.52.3 | Async runtime | Required by russh for SSH; streaming file downloads with reqwest need async; multi-node cluster deploys benefit from concurrent SSH sessions per node |
| `serde` + `serde_derive` | 1.0.228 | Serialization framework | Foundation for TOML config deserialization; zero-cost at runtime via compile-time codegen |
| `toml` | 1.1.2 | TOML config parsing | Official `toml-rs` crate; serde-compatible; `toml::from_str` directly deserializes into Rust structs; spec 1.1.0 compliant |
| `russh` | 0.61.2 | SSH client for cluster node operations | Pure Rust, async/Tokio-native; no native C library dependency (no libssh2); supports password + pubkey + OpenSSH certificate auth; includes SCP/SFTP via `russh-sftp`; used in production by Warp terminal |
| `russh-sftp` | 2.3.0 | SFTP file transfer to remote nodes | Companion crate to russh; needed to push the DM installer `.bin` to remote nodes before executing it |
| `reqwest` | 0.13.4 | Download DM installer package from official site | Async streaming download (`bytes_stream`) + progress reporting; TLS by default; handles redirects automatically |
| `anyhow` | 1.0.102 | Application-level error handling | Best for binary/application code; `context()` chains add location info without boilerplate; compatible with `?` operator everywhere |
| `thiserror` | 2.0.18 | Typed error enums for library-style modules | Use `#[derive(Error)]` for structured error types in SSH, config, and download modules; pairs with `anyhow` at the boundary |
### Supporting Libraries
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `indicatif` | 0.18.4 | Progress bars and spinners | During file download (bytes received/total), during remote SSH command execution (spinner), during DM installer steps |
| `console` | 0.16.3 | Terminal colors, styled output | Status messages (`[OK]`, `[ERROR]`, `[WARN]`); ANSI detection is automatic, falls back to plain text on non-TTY (pipes/CI) |
| `tracing` | 0.1.44 | Structured async-aware logging | Async-aware spans work correctly with tokio; `--verbose` flag maps to `RUST_LOG` filter; prefer over `log` crate in async code |
| `tracing-subscriber` | 0.3.23 | Log output formatting | `EnvFilter` + `fmt` layer gives `RUST_LOG=debug` support; human-readable output in dev, JSON in CI |
| `flate2` | 1.1.9 | Decompress DM installer ISO/tar.gz | DM packages are distributed as `.iso` or `.tar.gz`; needed to extract before running `DMInstall.bin` |
| `tar` | 0.4.46 | Unpack tar archives | Works with `flate2` for `.tar.gz` extraction |
| `sha2` | 0.11.0 | Verify downloaded package integrity | Compute SHA-256 of downloaded `.bin`/`.iso` and compare against manifest; prevents corrupted-install failures |
| `tempfile` | 3.27.0 | Safe temporary directory management | Create temp dirs for XML response files, extracted installers; auto-cleanup on drop |
| `clap_complete` | 4.6.5 | Shell completion generation | Generates bash/zsh/fish completions; ship via `dm-installer completions bash` subcommand |
| `tokio-util` | latest | Codec utilities for streaming | `tokio_util::io::ReaderStream` bridges sync reads into tokio streams when needed |
### Development Tools
| Tool | Purpose | Notes |
|------|---------|-------|
| `cross` | Cross-compile to Linux x86_64/aarch64 and Windows | `cargo install cross --git https://github.com/cross-rs/cross`; uses Docker images with pre-configured toolchains; drop-in `cargo` replacement |
| `cargo-nextest` | Fast parallel test runner | Significantly faster than `cargo test`; better output for integration tests with long setup |
| `cargo-dist` | Release binary distribution | Generates GitHub Actions CI, `curl \| sh` bootstrap script, and platform tarballs automatically; used by projects like uv and Rye |
## DM Silent Installation Integration
## Cargo.toml Dependencies
## curl | sh Bootstrap Pattern
## Alternatives Considered
| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| `russh` | `ssh2` (libssh2 bindings) | If you need broad compatibility with legacy SSH server quirks; but `ssh2` is C-FFI which complicates cross-compilation and has had no significant releases since Feb 2025 |
| `russh` | `async-ssh2-tokio` | Thin async wrapper around libssh2; simpler API but still pulls in C dependency; not needed if russh covers the use case |
| `reqwest` | `ureq` | If you want a pure blocking HTTP client with minimal dependencies and no async; fine for a simpler installer, but we need async for concurrent SSH + download operations |
| `anyhow` | `eyre` | `eyre` has richer error reporting hooks; use if you want custom colored error output (SpanTrace, etc.); `anyhow` is sufficient for this tool |
| `cargo-dist` | Hand-written release CI | Only if cargo-dist's opinions conflict with your release infra; cargo-dist covers 95% of the work |
| `tracing` | `log` + `env_logger` | `log` is simpler but has no span support, which makes async debugging harder; tokio async code benefits from tracing spans |
## What NOT to Use
| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `ssh2` (libssh2 bindings) | C FFI dependency makes cross-compilation painful, especially for Linux ARM and Windows targets; last meaningful update February 2025, maintenance uncertain | `russh` |
| `native-tls` feature in reqwest | Depends on OpenSSL (Linux) or SecureTransport (macOS) / SChannel (Windows); complex to cross-compile; linker errors common on minimal containers | `rustls-tls` feature in reqwest |
| `openssl` crate directly | Same cross-compilation problem as native-tls; requires OpenSSL headers in cross-compilation sysroot | Use rustls via `rustls` or indirectly via reqwest/russh with ring feature |
| `structopt` | Superseded by clap v4's derive API; `structopt` is now archived/maintenance-only | `clap` with `features = ["derive"]` |
| `serde_yaml` / YAML config | TOML is already decided (PROJECT.md constraint); YAML has footguns (Norway problem, implicit typing) | `toml` |
| Embedding full binary in base64 in shell script | Bloats the bootstrap script, breaks incremental updates, not cache-friendly | Separate binary download via `cargo-dist` pattern |
## Stack Patterns by Variant
- The downloaded binary runs with `--defaults` flag
- No SSH involved; process execution is local only
- Use `std::process::Command` (sync, no tokio needed for this path if you build a separate minimal binary — but a single binary with tokio is fine)
- Write XML response file to `tempfile::TempDir`, invoke `DMInstall.bin -q <path>`
- TOML config lists all nodes with SSH credentials
- Use `tokio::spawn` per-node for concurrent deployment
- russh: connect → upload installer binary via SFTP → execute `DMInstall.bin -q <xml>` → stream stdout back for logging
- Post-install: push `dm.ini`, `dmmal.ini`, `dmarch.ini`, `dmdcr.ini` config files per node role via SFTP
- DM provides `setup.exe` with `/q /XML <path>` flags (verify against DM Windows docs)
- Cross-compile target: `x86_64-pc-windows-gnu` (via `cross`)
- Process spawning uses `std::process::Command` on all platforms — same code, different binary names
- Check `nix::unistd::getuid()` (from `nix` crate) or `std::env::var("USER") == "root"`
- If not root: re-execute via `sudo` using `std::process::Command::new("sudo").arg(current_exe).args(original_args)`
- This is the same pattern `rustup-init.sh` uses for the bootstrap script
## Version Compatibility
| Package | Compatible With | Notes |
|---------|-----------------|-------|
| `russh 0.61.2` | `russh-sftp 2.3.0` | Must use matching major versions; `russh-sftp` wraps russh channels directly |
| `tokio 1.52.3` | `russh 0.61.2`, `reqwest 0.13.4` | All require tokio 1.x; no conflict |
| `clap 4.6.1` | `clap_complete 4.6.5` | Major version must match; 4.x throughout |
| `serde 1.0.228` | `toml 1.1.2`, `serde_derive 1.0.228` | Stable 1.x; all compatible |
| `reqwest 0.13.4` | `tokio 1.x` | reqwest 0.13 dropped hyper 0.x, uses hyper 1.x internally; no issue |
| `thiserror 2.0.18` | `anyhow 1.0.102` | Compatible; `thiserror` errors implement `std::error::Error` which `anyhow::Error` accepts |
## Sources
- `crates.io` API (verified 2026-06-12) — version numbers for all crates
- [clap docs.rs](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) — derive macro, subcommand patterns (HIGH confidence, Context7 verified)
- [russh GitHub eugeny/russh](https://github.com/Eugeny/russh) — SSH client API, SFTP examples (HIGH confidence, Context7 verified)
- [reqwest GitHub seanmonstar/reqwest](https://github.com/seanmonstar/reqwest) — streaming download, rustls-tls feature (HIGH confidence, Context7 verified)
- [toml docs.rs](https://docs.rs/toml/latest/) — `toml::from_str` deserialization (HIGH confidence, Context7 verified)
- [thiserror GitHub dtolnay/thiserror](https://github.com/dtolnay/thiserror) — derive Error macro (HIGH confidence, Context7 verified)
- [DM8 silent install — Tencent Cloud community](https://cloud.tencent.com/developer/article/2373070) — XML response file format, `-q` flag (MEDIUM confidence, community docs)
- [DM8 silent install — CSDN](https://blog.csdn.net/qq_37822702/article/details/135692094) — corroborating XML parameters (MEDIUM confidence)
- [cross-rs cross-compilation](https://kx.cloudingenium.com/en/cross-rust-cross-compile-any-target-docker-guide/) — cross tool usage for multi-target builds (MEDIUM confidence, verified against rustup docs)
- [rustup curl|sh pattern](https://rust-lang.github.io/rustup/installation/other.html) — bootstrap script pattern (HIGH confidence, official Rust docs)
<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->
## Conventions

**Naming & Organization**
- `snake_case` for modules and functions (`load_config`, `validate_install_config`, `load_standalone_specific`)
- `PascalCase` for public structs/enums (`InstallConfig`, `CommonConfig`, `DwClusterConfig`, `NodeRole`)
- Enums serialize to kebab/lowercase via `#[serde(rename_all = "lowercase")]` (`src/config/dw.rs:11`)
- Inline Chinese comments for domain-specific logic (platform detection, archive modes, backup policies)

**Error Handling**
- `anyhow::Result` + `Context`/`bail!` for application code (`main.rs`, `config/mod.rs`, `install/`)
- `thiserror` for structured domain errors with `#[source]` chains (`src/ssh/error.rs`)
- Semantic validation failures use `bail!("message")`, distinct from I/O errors (`src/config/mod.rs`)

**Tests**
- Inline `#[cfg(test)] mod tests` at the bottom of the module under test, not a separate test tree
- `tempfile::NamedTempFile` for config-parsing fixtures
- Test names follow `test_<function>_<scenario>`

**Comments**
- `///` doc comments only for high-value public API context — used sparingly
- `//` inline comments for non-obvious logic (platform heuristics, archive semantics)
- `// ──` separator lines for logical sections within large config structs

**Module Organization**
- Each feature area has a `mod.rs` entry point; submodules stay private unless re-exported
- Single-file modules for focused concerns (e.g. `ssh/error.rs` is errors only)
- Const strings for file paths/keys (`CONFIG_FILE`, checkpoint file names)

**Async**
- `#[tokio::main]`; async functions return `anyhow::Result<()>`
- Trait objects (`&dyn CommandRunner`) abstract local vs. SSH execution so the same step logic runs in both standalone and cluster paths
<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->
## Architecture

**Module layout** (`src/`)
- `main.rs` — async entry point; loads config, dispatches to `install::standalone` or `install::dw`
- `cli.rs` — clap CLI parsing (`install`/`validate`/`init`/`self-update` subcommands)
- `config/` — `mod.rs` (common config + `TryFrom` validation), `dw.rs` (cluster config structs), `ssh.rs` (SSH credentials)
- `download/` — installer source resolution: `http.rs` (reqwest streaming download), `select.rs` (platform/version selection), `versions.rs` (`versions.txt` parsing)
- `install/` — orchestration:
  - `standalone/` — single-host `[1/10]`–`[10/10]` step sequence + JSON checkpoint for resume-on-failure
  - `dw/` — multi-node cluster orchestration: connection pool, per-node config rendering (`config_dist.rs`, `config_files.rs`), provisioning, startup, post-setup, HA data sync, per-node checkpoint
  - `steps/` — shared step implementations (preflight, env setup, package download/extract, silent install, dminit, archive, backup, service, param tuning, SQL audit log) used by both standalone and cluster paths
  - `advisory.rs` — preflight warnings/user confirmations
  - `remote_common.rs` — SSH session setup/teardown shared by standalone-remote and cluster paths
- `ssh/` — `CommandRunner` trait abstracting local (`local.rs`, sync `std::process`) vs. remote (`session.rs`, async russh) execution; `error.rs` for structured SSH errors; `mock.rs` for tests
- `platform.rs` — OS/arch/CPU detection for version matching
- `ui.rs` — step headers, log levels, progress bars

**Data flow**
1. CLI parse (`main.rs`) → `config::load_config()` reads `config.toml` + a type-specific file (`standalone.toml` or `dw.toml`)
2. Raw TOML → `TryFrom<CommonConfigRaw>` (mutual exclusivity checks) → `validate_install_config()` (semantic checks: ports, paths, time formats)
3. Dispatch by config type to `install::standalone::run()` or `install::dw::run()`
4. Standalone: load/create checkpoint → run steps in order, skipping any already marked done → save checkpoint after each step
5. Cluster: connect to all nodes via russh → run the same step sequence per node (parallel via `tokio::spawn`), with cluster-aware logic for primary→standby sync → track per-node step completion in the cluster checkpoint

**Key decisions**
- Config-driven (TOML), not interactive prompts — config.toml + a type-specific file
- `CommandRunner` trait makes step code identical for standalone and SSH-remote/cluster execution
- Checkpoints make installs resumable after failure, both per-host and per-node-in-cluster
- Async throughout (tokio) — required for concurrent SSH sessions and streaming downloads
<!-- GSD:architecture-end -->

<!-- GSD:skills-start source:skills/ -->
## Project Skills

No project skills found. Add skills to any of: `.claude/skills/`, `.agents/skills/`, `.cursor/skills/`, `.github/skills/`, or `.codex/skills/` with a `SKILL.md` index file.
<!-- GSD:skills-end -->

<!-- GSD:workflow-start source:GSD defaults -->
## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:
- `/gsd-quick` for small fixes, doc updates, and ad-hoc tasks
- `/gsd-debug` for investigation and bug fixing
- `/gsd-execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->



<!-- GSD:profile-start -->
## Developer Profile

> Profile not yet configured. Run `/gsd-profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->
