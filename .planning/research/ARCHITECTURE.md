# Architecture Research

**Domain:** Multi-mode database installer CLI (Rust)
**Researched:** 2026-06-12
**Confidence:** HIGH

## Standard Architecture

### System Overview

```
┌──────────────────────────────────────────────────────────────────┐
│                         Entry Points                              │
│  ┌──────────────────────┐        ┌──────────────────────────┐    │
│  │  curl | sh bootstrap │        │  dm-installer binary     │    │
│  │  (install.sh)        │        │  clap subcommands        │    │
│  └──────────┬───────────┘        └──────────────┬───────────┘    │
│             │ downloads & execs                  │               │
└─────────────┼──────────────────────────────────-┼───────────────┘
              │                                    │
┌─────────────▼────────────────────────────────────▼───────────────┐
│                      CLI Layer (clap derive)                       │
│  Commands: install [--config] | deploy <config.toml> | status     │
└──────────────────────────────┬───────────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────┐
│                     Orchestrator Layer                             │
│  Reads topology config → resolves Plan → executes Phase pipeline   │
│  ┌───────────────────────────────────────────────────────────┐   │
│  │  Phase Pipeline                                            │   │
│  │  Download → Verify → Push → Install → Init → Configure    │   │
│  └───────────────────────────────────────────────────────────┘   │
└────────┬─────────────────┬────────────────────────────────────────┘
         │                 │
┌────────▼──────┐  ┌───────▼────────────────────────────────────────┐
│ Downloader    │  │  Executor (local / SSH remote)                  │
│  reqwest +    │  │  ┌───────────────┐  ┌──────────────────────┐   │
│  indicatif    │  │  │ LocalExecutor │  │ SshExecutor          │   │
│  SHA256 check │  │  │ std::process  │  │ russh + russh-sftp   │   │
└───────────────┘  │  └───────────────┘  │ parallel tokio tasks │   │
                   │                     └──────────────────────┘   │
                   └────────────────────────────────────────────────┘
                                     │
           ┌─────────────────────────▼──────────────────────────────┐
           │                Config / Schema Layer                     │
           │  serde + toml → TopologyConfig enum (4 variants)        │
           │  + GlobalConfig (shared params collapsed via #[flatten]) │
           └────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Responsibility | Implementation |
|-----------|---------------|----------------|
| `install.sh` bootstrap | Detect platform, download binary, exec | POSIX sh, single function-then-call guard |
| CLI layer | Argument parsing, subcommand dispatch | `clap` derive API, `Commands` enum |
| TopologyConfig | Deserialize and validate TOML | `serde`, internally-tagged enum `[topology] type = "standalone"` |
| Orchestrator | Build phase plan from config, drive execution | Pure Rust structs, holds `Vec<Phase>` |
| Downloader | Fetch DM installer `.zip`/`.bin` with progress | `reqwest` streaming + `indicatif` + `sha2` |
| LocalExecutor | Run commands on the control machine | `std::process::Command` / `tokio::process` |
| SshExecutor | Upload files and run commands on remote nodes | `russh` + `russh-sftp`, `tokio::spawn` per node |
| PhaseRunner | Execute one phase across assigned nodes in parallel | `tokio::join_all` over executor futures |
| ConfigWriter | Render DM config files (dm.ini, dmmal.ini, etc.) | `minijinja` or `format!` string templates |

## Recommended Project Structure

```
src/
├── main.rs                  # clap parse → dispatch to commands
├── cli/
│   ├── mod.rs               # Commands enum, Cli struct
│   ├── install.rs           # `dm-installer install` (curl|sh path)
│   └── deploy.rs            # `dm-installer deploy <config.toml>`
├── config/
│   ├── mod.rs               # re-exports
│   ├── schema.rs            # TopologyConfig, GlobalConfig, node structs
│   ├── standalone.rs        # StandaloneConfig specifics
│   ├── primary_standby.rs   # PrimaryStandbyConfig specifics
│   ├── dsc.rs               # DscConfig specifics
│   └── dpc.rs               # DpcConfig specifics
├── plan/
│   ├── mod.rs               # Plan struct, Phase enum, build_plan()
│   └── phases.rs            # Phase definitions: Download, Verify, Push, Install, Init, Configure
├── executor/
│   ├── mod.rs               # Executor trait
│   ├── local.rs             # LocalExecutor
│   └── ssh.rs               # SshExecutor (russh-based)
├── downloader/
│   └── mod.rs               # fetch_installer(), verify_sha256()
├── render/
│   └── mod.rs               # render_dm_ini(), render_dmmal_ini(), etc.
└── error.rs                 # unified Error / Result type (thiserror)
```

### Structure Rationale

- **`cli/`:** Thin dispatch layer only — no business logic, just parses args and calls plan builder.
- **`config/`:** Each topology variant in its own file; `schema.rs` ties them together via `TopologyConfig`. Keeps diff noise low as modes evolve independently.
- **`plan/`:** The orchestrator — topology-agnostic after `build_plan()`. Phase order is encoded here, not scattered across config files.
- **`executor/`:** The `Executor` trait is the seam between local and SSH execution. Every phase step goes through this trait — allows unit testing with a `MockExecutor`.
- **`render/`:** DM config files are plain text with known formats. Keep rendering logic here so phases stay free of string manipulation.
- **`error.rs`:** `thiserror`-derived `InstallerError` with variants per subsystem (Download, Ssh, Config, etc.).

## Architectural Patterns

### Pattern 1: Internally-Tagged Topology Enum

**What:** A single `TopologyConfig` enum with `#[serde(tag = "type")]` dispatches to four topology variants. Shared params live in a `GlobalConfig` struct that each variant `#[serde(flatten)]`-includes.

**When to use:** Four topology modes share ~80% of their config surface (install path, port, charset, passwords). Avoid duplicating those fields in each variant.

**Trade-offs:** `#[serde(flatten)]` on struct fields works cleanly in serde; avoid flattening *enum variants* directly (known serde limitation). The pattern below sidesteps that limitation.

**Example:**
```toml
# config.toml
[global]
install_dir = "/opt/dmdbms"
port        = 5236
db_name     = "DAMENG"
charset     = "UTF-8"
password    = "Dameng@123"

[topology]
type = "primary_standby"

[[topology.nodes]]
role = "primary"
host = "192.168.1.10"
ssh_user = "root"
ssh_key  = "~/.ssh/id_rsa"

[[topology.nodes]]
role = "standby"
host = "192.168.1.11"
ssh_user = "root"
ssh_key  = "~/.ssh/id_rsa"
```

```rust
// config/schema.rs
#[derive(Deserialize)]
pub struct InstallerConfig {
    pub global: GlobalConfig,
    pub topology: TopologyConfig,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TopologyConfig {
    Standalone(StandaloneConfig),
    PrimaryStandby(PrimaryStandbyConfig),
    Dsc(DscConfig),
    Dpc(DpcConfig),
}

#[derive(Deserialize)]
pub struct GlobalConfig {
    pub install_dir: PathBuf,
    pub port: u16,
    pub db_name: String,
    pub charset: String,
    pub password: String,
}
```

### Pattern 2: Phase Pipeline with Executor Trait

**What:** Orchestrator converts `TopologyConfig` into an ordered `Vec<Phase>`. Each `Phase` carries a list of `NodeTask` items (target node + command/file). A `PhaseRunner` iterates phases sequentially but fans out node tasks in parallel within each phase.

**When to use:** Always — this is the core execution model. Phases enforce build order; parallelism within a phase gives speed on cluster deployments.

**Trade-offs:** Sequential phases add latency vs fully parallel; correctness requires it (can't configure cluster before binaries are installed). Phase granularity is a tuning knob.

**Example:**
```rust
// executor/mod.rs
#[async_trait]
pub trait Executor: Send + Sync {
    async fn run_cmd(&self, cmd: &str) -> Result<Output>;
    async fn upload(&self, local: &Path, remote: &Path) -> Result<()>;
}

// plan/mod.rs
pub struct Phase {
    pub name: &'static str,
    pub tasks: Vec<(Arc<dyn Executor>, NodeTask)>,
}

pub async fn run_plan(phases: Vec<Phase>) -> Result<()> {
    for phase in phases {
        let futures: Vec<_> = phase.tasks
            .into_iter()
            .map(|(exec, task)| tokio::spawn(task.run(exec)))
            .collect();
        futures::future::try_join_all(futures).await?;
    }
    Ok(())
}
```

### Pattern 3: curl | sh Bootstrap via Shell-Downloads-Binary

**What:** A POSIX shell script (`install.sh`) acts solely as a platform detector and binary fetcher — identical to rustup's approach. The script detects OS + arch, constructs the download URL, downloads the pre-built `dm-installer` binary, and execs it. All real logic stays in the Rust binary.

**When to use:** The only viable `curl | sh` pattern for a Rust tool. Avoids embedding complex install logic in shell (fragile, hard to test).

**Trade-offs:** Requires hosting pre-built binaries for each target (Linux x86_64, Linux aarch64, Windows x86_64). The shell script itself must be safe against partial downloads — wrap the real work in a `main()` function called only at the last line.

**Example:**
```sh
#!/usr/bin/env sh
# install.sh — detect platform, download dm-installer binary
set -eu

BASE_URL="https://releases.example.com/dm-installer"

detect_target() {
  OS=$(uname -s | tr '[:upper:]' '[:lower:]')
  ARCH=$(uname -m)
  case "${ARCH}" in
    x86_64)  ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) echo "Unsupported arch: ${ARCH}" >&2; exit 1 ;;
  esac
  echo "${OS}-${ARCH}"
}

main() {
  TARGET=$(detect_target)
  URL="${BASE_URL}/${TARGET}/dm-installer"
  DEST="${HOME}/.local/bin/dm-installer"
  mkdir -p "$(dirname "${DEST}")"
  curl -fsSL "${URL}" -o "${DEST}"
  chmod +x "${DEST}"
  "${DEST}" install "$@"
}

main "$@"
```

## Data Flow

### deploy Command Flow

```
User: dm-installer deploy cluster.toml
         │
         ▼
   CLI parse (clap)
         │
         ▼
   load_config(path) → InstallerConfig { global, topology }
         │
         ▼
   build_plan(&config) → Vec<Phase>
   (topology variant → phase list with node assignments)
         │
         ▼
   Phase: Download
     downloader::fetch_installer(arch) → /tmp/dm8.zip   (local only)
     downloader::verify_sha256(path, expected)
         │
         ▼
   Phase: Push (cluster modes only)
     SshExecutor[node1].upload(/tmp/dm8.zip, /tmp/) ─┐  parallel
     SshExecutor[node2].upload(/tmp/dm8.zip, /tmp/) ─┘  tokio::join_all
         │
         ▼
   Phase: Install
     Executor[nodeN].run_cmd("unzip /tmp/dm8.zip && ./DMInstall.bin -q") ─┐ parallel
         │
         ▼
   Phase: Init (dminit)
     render::render_dminit_args(&config, role) → arg string
     Executor[nodeN].run_cmd("dminit PATH=... PORT_NUM=...") ─┐ parallel
         │
         ▼
   Phase: Configure (cluster modes only)
     render::render_dmmal_ini(&config) → dmmal.ini content
     render::render_dmarch_ini(&config) → dmarch.ini content
     render::render_dmwatcher_ini(&config) → dmwatcher.ini content
     Executor[nodeN].upload(rendered_files) + run_cmd("systemctl start DmServiceDMSERVER")
         │
         ▼
   Phase: Verify
     Executor[nodeN].run_cmd("disql SYSDBA/... -e 'select status$ from v$instance'")
         │
         ▼
   Report: OK / per-node error summary
```

### Configuration File Rendering Flow

```
TopologyConfig variant
    │
    ├── Standalone      → renders: dm.ini only
    ├── PrimaryStandby  → renders: dm.ini, dmmal.ini, dmarch.ini, dmwatcher.ini, dmmonitor.ini
    ├── Dsc             → renders: dm.ini, dmdcr.ini, dmasm.ini (shared storage config)
    └── Dpc             → renders: dm.ini (DPC_MODE param), + BP/MP specific configs
```

### SSH Execution Model

```
Per-node, per-phase:
  SshExecutor::new(host, port, user, auth)
      │ russh::client::connect()
      ▼
  connection pool (one persistent connection per node per plan run)
      │
      ├── upload(local, remote) → russh-sftp subsystem → remote file
      └── run_cmd(cmd)          → russh channel → stdout/stderr capture
                                              → exit code check
```

## Build Order (Phase Dependencies)

```
1. Download        — fetches the DM .zip to the control machine (no node contact)
2. Verify          — SHA256 check before touching any node
3. Push            — uploads .zip to each remote node (SSH upload, parallel)
4. Install binary  — runs DMInstall.bin -q on each node (parallel)
5. Init instance   — runs dminit with per-role parameters (parallel, role-aware)
6. Write configs   — renders and uploads dm.ini, dmmal.ini etc. (parallel)
7. Start services  — systemctl enable + start DmService* (sequential: primary first)
8. Cluster join    — for DSC/DPC: registers nodes with the cluster coordinator (sequential)
9. Health check    — queries v$instance status on each node (parallel)
```

Step 7 MUST be sequential for primary-standby: start primary first, verify it's up, then start standby. Steps 3–6 are safely parallel across nodes.

## Anti-Patterns

### Anti-Pattern 1: Topology Logic Inside Config Structs

**What people do:** Put `impl PrimaryStandbyConfig { fn run(&self) {...} }` directly on config types, mixing deserialization with execution logic.

**Why it's wrong:** Config types become untestable god-objects. Phases can't be reordered, retried, or mocked independently. Adding a new topology variant requires touching execution code in config files.

**Do this instead:** Config structs are pure data (derive `Deserialize`, nothing else). The `plan::build_plan()` function interprets config and produces a topology-agnostic `Vec<Phase>`. Execution lives entirely in `PhaseRunner`.

### Anti-Pattern 2: Separate TOML Schemas Per Topology

**What people do:** Four completely independent TOML schemas, each with all fields duplicated (install_dir, port, charset, passwords appear in all four).

**Why it's wrong:** A change to a shared field (e.g. renaming `password` to `admin_password`) requires updating four schemas and four deserialization paths. Users have no single reference for shared params.

**Do this instead:** `[global]` table holds all shared params. `[topology]` table is tagged with `type` and holds only topology-specific fields (node lists, roles, shared storage paths). Serde's internally-tagged enum handles dispatch cleanly.

### Anti-Pattern 3: Spawning a New SSH Connection Per Command

**What people do:** `SshExecutor::run_cmd()` opens a new SSH connection each time, runs the command, disconnects.

**Why it's wrong:** SSH handshake is expensive (~100-500ms). A 9-phase installation across 4 nodes with 5 commands each = 180 unnecessary reconnects, adding minutes to total runtime. Also breaks if the remote side has connection rate limits.

**Do this instead:** Establish one SSH connection per node at plan start, store it in `SshExecutor`, reuse it across all phases. Close connections in a cleanup step after the final phase.

### Anti-Pattern 4: Shell Logic in install.sh Beyond Platform Detection

**What people do:** Put the entire install workflow into `install.sh` — unzip, run dminit, write config files, all in bash.

**Why it's wrong:** Shell scripts are hard to test, brittle on edge cases (quoting, paths with spaces, non-bash shells), and impossible to cross-compile to Windows. Any Windows support dies immediately.

**Do this instead:** `install.sh` does one thing: detect platform, download the Rust binary, exec it. All logic lives in the binary which is testable with Rust's test framework.

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| DM official download | `reqwest` GET to known URL pattern, streaming to file | URL may change; make it configurable with a compiled-in default |
| Remote nodes (SSH) | `russh` + `russh-sftp` async client | Auth via key file or password; key preferred for automation |
| DM database (health check) | `run_cmd("disql ...")` over SSH or locally | No native Rust driver needed for install-time checks |
| systemd | `run_cmd("systemctl enable/start ...")` over SSH | Windows uses service control; executor trait hides this |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| CLI → Orchestrator | Function call with `InstallerConfig` value | CLI has no knowledge of phases |
| Orchestrator → Executor | `Executor` trait async methods | Orchestrator never imports russh or std::process directly |
| Orchestrator → Render | Function calls returning `String` | No file I/O in render layer; executor does the upload |
| Downloader → Plan | Returns `PathBuf` to downloaded file | Path passed into Push phase as a `NodeTask` input |
| Config → Plan | `build_plan(&InstallerConfig) -> Vec<Phase>` | Single function, pure — testable without network or SSH |

## Cross-Platform Build Strategy

Target matrix for CI (GitHub Actions matrix strategy):

| Target | Tool | Notes |
|--------|------|-------|
| `x86_64-unknown-linux-musl` | `cross` | Static binary, runs on all x86 Linux |
| `aarch64-unknown-linux-musl` | `cross` | ARM64 Linux (Kunpeng, Raspberry Pi, AWS Graviton) |
| `x86_64-pc-windows-msvc` | native runner | Windows; avoid GNU toolchain for Windows |

Key constraint: **avoid OpenSSL**. Use `rustls` (via `reqwest`'s `rustls-tls` feature and `russh`'s `rust-crypto` feature) to enable cross-compilation without a C toolchain or system libraries.

```toml
# Cargo.toml
[dependencies]
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "stream"] }
russh = { version = "0.44", features = ["rs-crypto"] }
```

Windows-specific: `LocalExecutor` on Windows runs `std::process::Command` with `.exe` awareness. SSH-based remote operations target Linux nodes only (DM cluster nodes are always Linux). The Windows build is for the control machine running the installer CLI, not the database nodes.

## Sources

- TiUP topology spec: https://github.com/pingcap/tiup/blob/master/pkg/cluster/spec/spec.go
- TiUP cluster topology reference: https://docs.pingcap.com/tidb/stable/tiup-cluster-topology-reference/
- OceanBase OBD architecture: https://deepwiki.com/oceanbase/obdeploy/2-getting-started
- rustup-init.sh bootstrap pattern: https://github.com/rust-lang/rustup/blob/main/rustup-init.sh
- async-ssh2-tokio: https://lib.rs/crates/async-ssh2-tokio
- russh: https://github.com/Eugeny/russh
- massh (parallel SSH): https://docs.rs/massh
- serde enum representations: https://serde.rs/enum-representations.html
- cross-compilation with cross-rs: https://blog.ediri.io/how-to-cross-compile-your-rust-applications-using-cross-rs-and-github-actions
- 达梦 dminit 参数: https://eco.dameng.com/document/dm/zh-cn/pm/dminit-parameters.html
- 达梦配置文件说明: https://eco.dameng.com/document/dm/zh-cn/pm/configuration-description.html
- 达梦主备集群 dmmal.ini/dmarch.ini: https://eco.dameng.com/community/article/7a4a1969a43ba09eb683c14bfb8ee7ec

---
*Architecture research for: 达梦数据库安装器 (dm-database-installer)*
*Researched: 2026-06-12*
