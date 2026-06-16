# Stack Research

**Domain:** Rust CLI installer tool for DM (达梦) database
**Researched:** 2026-06-12
**Confidence:** HIGH (core crates verified via crates.io API + Context7; DM silent install verified via official community docs)

---

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

---

## DM Silent Installation Integration

DM8's installer (`DMInstall.bin` on Linux, `setup.exe` on Windows) supports **unattended/silent mode** via:

```
./DMInstall.bin -q /path/to/auto_install.xml
```

The XML response file controls all installation parameters. This tool generates this XML from the TOML config and invokes the installer via `std::process::Command`.

**Key XML parameters to template:**

```xml
<?xml version="1.0"?>
<DATABASE>
  <LANGUAGE>ZH</LANGUAGE>
  <TIME_ZONE>+08:00</TIME_ZONE>
  <INSTALL_TYPE>0</INSTALL_TYPE>       <!-- 0=all, 1=server, 2=client -->
  <INSTALL_PATH>/opt/dameng</INSTALL_PATH>
  <INIT_DB>Y</INIT_DB>
  <DB_PARAMS>
    <PATH>/opt/dameng/data</PATH>
    <DB_NAME>DAMENG</DB_NAME>
    <INSTANCE_NAME>DMSERVER</INSTANCE_NAME>
    <PORT_NUM>5236</PORT_NUM>
    <CHARSET>1</CHARSET>              <!-- 0=GB18030, 1=UTF-8 -->
    <PAGE_SIZE>8</PAGE_SIZE>           <!-- KB: 4, 8, 16, 32 -->
    <EXTENT_SIZE>16</EXTENT_SIZE>
    <LOG_SIZE>256</LOG_SIZE>
    <SYSDBA_PWD>SYSDBA_123</SYSDBA_PWD>
    <SYSAUDITOR_PWD>SYSAUDITOR_123</SYSAUDITOR_PWD>
    <CREATE_DB_SERVICE>Y</CREATE_DB_SERVICE>
    <STARTUP_DB_SERVICE>Y</STARTUP_DB_SERVICE>
  </DB_PARAMS>
</DATABASE>
```

**Linux privilege requirements:** The DM installer must run as `root` or via `sudo` to create the `dmdba` system user and register the systemd service. The tool must either run as root or invoke `sudo DMInstall.bin -q ...`. For the `curl | sh` flow on Linux, the bootstrap script should re-exec itself with `sudo` if not already root.

---

## Cargo.toml Dependencies

```toml
[dependencies]
clap = { version = "4.6.1", features = ["derive"] }
tokio = { version = "1.52.3", features = ["full"] }
serde = { version = "1.0.228", features = ["derive"] }
toml = "1.1.2"
russh = { version = "0.61.2", features = ["ring"] }
russh-sftp = "2.3.0"
reqwest = { version = "0.13.4", features = ["stream", "rustls-tls"], default-features = false }
anyhow = "1.0.102"
thiserror = "2.0.18"
indicatif = "0.18.4"
console = "0.16.3"
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
flate2 = "1.1.9"
tar = "0.4.46"
sha2 = "0.11.0"
tempfile = "3.27.0"

[dev-dependencies]
tokio = { version = "1.52.3", features = ["full", "test-util"] }
```

Note: `reqwest` uses `rustls-tls` (not `native-tls`) to avoid OpenSSL dependency issues on minimal Linux environments and Windows cross-compilation.

---

## curl | sh Bootstrap Pattern

The standard `curl | sh` bootstrap (model: `rustup`, `volta`, `cargo-dist`) works as:

1. A shell script hosted at a stable URL (e.g., `https://install.example.com/dm.sh`) is fetched
2. The script detects OS + architecture (`uname -s`, `uname -m`)
3. Downloads the correct pre-built binary from a GitHub Releases URL
4. Verifies checksum
5. Runs the binary with `--defaults` flag

`cargo-dist` generates this entire infrastructure automatically including the shell script template, GitHub Actions release workflow, and binary naming conventions.

The shell script itself is ~100 lines of POSIX sh — write it by hand or generate with `cargo-dist`. **Do not** embed the full Rust binary in the shell script (heredoc base64 style) — that approach bloats the script and defeats incremental updates.

---

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| `russh` | `ssh2` (libssh2 bindings) | If you need broad compatibility with legacy SSH server quirks; but `ssh2` is C-FFI which complicates cross-compilation and has had no significant releases since Feb 2025 |
| `russh` | `async-ssh2-tokio` | Thin async wrapper around libssh2; simpler API but still pulls in C dependency; not needed if russh covers the use case |
| `reqwest` | `ureq` | If you want a pure blocking HTTP client with minimal dependencies and no async; fine for a simpler installer, but we need async for concurrent SSH + download operations |
| `anyhow` | `eyre` | `eyre` has richer error reporting hooks; use if you want custom colored error output (SpanTrace, etc.); `anyhow` is sufficient for this tool |
| `cargo-dist` | Hand-written release CI | Only if cargo-dist's opinions conflict with your release infra; cargo-dist covers 95% of the work |
| `tracing` | `log` + `env_logger` | `log` is simpler but has no span support, which makes async debugging harder; tokio async code benefits from tracing spans |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `ssh2` (libssh2 bindings) | C FFI dependency makes cross-compilation painful, especially for Linux ARM and Windows targets; last meaningful update February 2025, maintenance uncertain | `russh` |
| `native-tls` feature in reqwest | Depends on OpenSSL (Linux) or SecureTransport (macOS) / SChannel (Windows); complex to cross-compile; linker errors common on minimal containers | `rustls-tls` feature in reqwest |
| `openssl` crate directly | Same cross-compilation problem as native-tls; requires OpenSSL headers in cross-compilation sysroot | Use rustls via `rustls` or indirectly via reqwest/russh with ring feature |
| `structopt` | Superseded by clap v4's derive API; `structopt` is now archived/maintenance-only | `clap` with `features = ["derive"]` |
| `serde_yaml` / YAML config | TOML is already decided (PROJECT.md constraint); YAML has footguns (Norway problem, implicit typing) | `toml` |
| Embedding full binary in base64 in shell script | Bloats the bootstrap script, breaks incremental updates, not cache-friendly | Separate binary download via `cargo-dist` pattern |

---

## Stack Patterns by Variant

**For the `curl | sh` single-node bootstrap:**
- The downloaded binary runs with `--defaults` flag
- No SSH involved; process execution is local only
- Use `std::process::Command` (sync, no tokio needed for this path if you build a separate minimal binary — but a single binary with tokio is fine)
- Write XML response file to `tempfile::TempDir`, invoke `DMInstall.bin -q <path>`

**For cluster (primary-standby / DSC / DPC) deployment:**
- TOML config lists all nodes with SSH credentials
- Use `tokio::spawn` per-node for concurrent deployment
- russh: connect → upload installer binary via SFTP → execute `DMInstall.bin -q <xml>` → stream stdout back for logging
- Post-install: push `dm.ini`, `dmmal.ini`, `dmarch.ini`, `dmdcr.ini` config files per node role via SFTP

**For Windows support:**
- DM provides `setup.exe` with `/q /XML <path>` flags (verify against DM Windows docs)
- Cross-compile target: `x86_64-pc-windows-gnu` (via `cross`)
- Process spawning uses `std::process::Command` on all platforms — same code, different binary names

**For privilege escalation on Linux:**
- Check `nix::unistd::getuid()` (from `nix` crate) or `std::env::var("USER") == "root"`
- If not root: re-execute via `sudo` using `std::process::Command::new("sudo").arg(current_exe).args(original_args)`
- This is the same pattern `rustup-init.sh` uses for the bootstrap script

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| `russh 0.61.2` | `russh-sftp 2.3.0` | Must use matching major versions; `russh-sftp` wraps russh channels directly |
| `tokio 1.52.3` | `russh 0.61.2`, `reqwest 0.13.4` | All require tokio 1.x; no conflict |
| `clap 4.6.1` | `clap_complete 4.6.5` | Major version must match; 4.x throughout |
| `serde 1.0.228` | `toml 1.1.2`, `serde_derive 1.0.228` | Stable 1.x; all compatible |
| `reqwest 0.13.4` | `tokio 1.x` | reqwest 0.13 dropped hyper 0.x, uses hyper 1.x internally; no issue |
| `thiserror 2.0.18` | `anyhow 1.0.102` | Compatible; `thiserror` errors implement `std::error::Error` which `anyhow::Error` accepts |

---

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

---

*Stack research for: DM database CLI installer (Rust)*
*Researched: 2026-06-12*
