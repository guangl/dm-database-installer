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
