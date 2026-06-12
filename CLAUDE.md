<!-- GSD:project-start source:PROJECT.md -->
## Project

**达梦数据库安装器 (dm-database-installer)**

一个 Rust CLI 工具，自动化安装达梦数据库。面向开发者，提供 `curl | sh` 一行命令快速拉起单机环境；面向 DBA/运维，通过 TOML 配置文件精细控制单机、主备、DSC 集群、DPC 集群的完整部署流程，支持 SSH 远程操作多节点。

**Core Value:** 开发者一行命令搞定本地达梦环境，DBA 用配置文件完成生产集群部署——两类用户都不需要手动操作达梦原生安装程序。

### Constraints

- **Tech Stack**: Rust — 已确定，性能和跨平台部署需求
- **Config Format**: TOML — Rust 生态首选，层级嵌套自然
- **Version Strategy**: 固定单版本 — 官网最新，无需版本矩阵
- **Distribution (单行命令)**: `curl | sh` 风格 — 开发者最低摩擦体验
- **Cluster Execution**: 单点 SSH 远程推送 — 用户无需在每个节点手动操作
- **Platforms**: Linux (x86/ARM) + Windows — 两类场景都要覆盖
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

Conventions not yet established. Will populate as patterns emerge during development.
<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->
## Architecture

Architecture not yet mapped. Follow existing patterns found in the codebase.
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
