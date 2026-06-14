---
phase: 03-cluster
verified: 2026-06-13T10:00:00Z
status: human_needed
score: 5/5 must-haves verified
overrides_applied: 0
human_verification:
  - test: "在两台真实 Linux 节点（或 Docker sshd 容器）上运行 `RUST_LOG=info cargo run --bin dm-installer -- cluster deploy --config cluster.toml`"
    expected: "日志中出现两个节点的 [预检查] 条目 → [node:Primary][1/6] 序列 → '主节点就绪' 字样 → [node:Standby][5/6] 启动达梦备实例 → '集群部署完成'；主节点健康确认行早于备节点启动行"
    why_human: "需要真实 SSH 可达节点、DM 安装包、主备 TCP 实际连通才能端到端验证 CLUS-02 SC3 的运行时日志顺序（Plan 03 Task 4 manual-deferred，无真实双节点测试环境）"
---

# Phase 3: 主备集群 Verification Report

**Phase Goal:** 用户可通过一份 TOML 配置文件，在一台控制机上完成双节点主备集群的完整部署
**Verified:** 2026-06-13T10:00:00Z
**Status:** human_needed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | 用户运行 `dm-installer cluster deploy --config <toml>` CLI 子命令可被解析 | VERIFIED | `src/cli.rs` 含 `Commands::Cluster(ClusterArgs)` + `ClusterSubcommand::Deploy(ClusterDeployArgs)`；3 个 CLI 测试通过；`cargo run -- cluster deploy --help` 输出含 `--config <CONFIG>` |
| 2 | 未提供 --config 时 CLI 解析失败并报错 | VERIFIED | `cargo run -- cluster deploy` 报 `error: the following required arguments were not provided`；`test_cluster_deploy_requires_config` 测试通过 |
| 3 | cluster::run 编排完整六步序列：load → preflight → 并发 SFTP+install → dminit → 推 4 个 INI → 启主 → wait_tcp_ready(primary) → 主 disql SQL → 启备 → wait_tcp_ready(standby) → 备 disql SQL → 启 dmwatcher | VERIFIED | `src/cluster/mod.rs` 的 `run_with_runners` 分为 5 个子函数：`run_preflight → run_install_phase → run_distribute_phase → run_startup_phase → run_watcher_phase`；startup_phase 中 primary mount → health_check → primary disql → standby mount → health_check → standby disql 顺序可在代码 L134-L146 直接读取 |
| 4 | 有序启动可从 tracing 日志观察：primary 健康确认在 standby 启动之前 | VERIFIED (代码+单元测试) | `run_startup_phase` 在 L137 `tracing::info!("[node:{:?}] 主节点就绪")` 后才在 L143 `tracing::info!("[node:{:?}][5/6] 启动达梦备实例")`；`test_run_orders_primary_health_before_standby_start` 用 `#[tracing_test::traced_test]` + `logs_assert` 断言顺序，已通过 |
| 5 | 预检查失败时 cluster::run 在执行任何安装操作前 bail | VERIFIED | `run_preflight` 调用 `preflight::preflight_all_nodes(...).await?`，任一失败立即返回；`test_run_aborts_on_preflight_failure_before_install` 断言 primary exec_log 不含 dminit/dmserver/disql，已通过 |

**Score:** 5/5 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/config/cluster.rs` | ClusterConfig / NodeConfig / NodeRole / SshCredentials + load/validate | VERIFIED | 6 个 pub 项均存在；SshCredentials.password 有 `#[serde(skip_serializing, default)]`；9 个单元测试通过 |
| `src/cluster/mod.rs` | cluster::run 完整实现（非 stub） | VERIFIED | `pub async fn run(args: &ClusterDeployArgs) -> Result<()>` 实现完整；无 `unimplemented!` 字样 |
| `src/cluster/deploy.rs` | 7 个节点级编排函数 | VERIFIED | `build_dminit_args`, `upload_installer_and_install`, `run_dminit_remote`, `distribute_configs`, `start_dmserver_mount`, `configure_database_role`, `start_dmwatcher` 全部存在；5 个单元测试通过 |
| `src/cluster/ssh.rs` | SshError + CommandRunner + SshSession + TofuHandler + MockRunner | VERIFIED | 所有 pub 结构体/枚举/trait 存在；exec_log/sftp_log 访问器存在；4 个单元测试通过 |
| `src/cluster/preflight.rs` | 三项预检查 + preflight_all_nodes | VERIFIED | 5 个 pub 函数全部存在；使用 `Arc<dyn CommandRunner>` + `join_all`；5 个单元测试通过 |
| `src/cluster/health.rs` | wait_tcp_ready 异步函数 | VERIFIED | `pub async fn wait_tcp_ready` 存在；`POLL_INTERVAL = Duration::from_secs(3)`；2 个单元测试通过 |
| `src/cli.rs` | Commands::Cluster + ClusterDeployArgs | VERIFIED | `Cluster(ClusterArgs)` variant、`ClusterSubcommand::Deploy(ClusterDeployArgs)`、必填 `--config: PathBuf` 均存在 |
| `src/main.rs` | Commands::Cluster dispatch | VERIFIED | L29-L31 含 `cli::Commands::Cluster(args) => match &args.command { cli::ClusterSubcommand::Deploy(deploy_args) => cluster::run(deploy_args).await }` |
| `Cargo.toml` | russh 0.61.2 + russh-sftp 2.3.0 依赖 | VERIFIED | 依赖树无 openssl/native-tls；ring crypto backend |
| `tests/fixtures/cluster_valid.toml` | 完整集群 TOML fixture | VERIFIED | 含 `role = "primary"` 和 `role = "standby"` 各一次 |
| `tests/fixtures/cluster_invalid_no_primary.toml` | 无 primary 节点 fixture | VERIFIED | 不含 `role = "primary"`；`cargo run -- cluster deploy --config ...` 报 "必须恰好一个 primary 节点" |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/main.rs` | `src/cluster/mod.rs` | `Commands::Cluster` dispatch | WIRED | L29-31 直接调用 `cluster::run(deploy_args)` |
| `src/cluster/mod.rs` | `src/cluster/deploy.rs` | `deploy::` 前缀调用 | WIRED | `run_install_phase` 调用 `deploy::upload_installer_and_install`、`deploy::run_dminit_remote`；`run_startup_phase` 调用 `deploy::start_dmserver_mount`、`deploy::configure_database_role`；`run_watcher_phase` 调用 `deploy::start_dmwatcher` |
| `src/cluster/mod.rs` | `src/cluster/health.rs` | `health::wait_tcp_ready` | WIRED | `run_with_runners_impl` L34 `health::wait_tcp_ready` 作为 `health_check_fn` 传入 |
| `src/cluster/mod.rs` | `src/cluster/preflight.rs` | `preflight::preflight_all_nodes` | WIRED | `run_preflight` L74 直接调用 |
| `src/cluster/preflight.rs` | `src/cluster/ssh.rs` | `CommandRunner` trait 注入 | WIRED | `check_sudo_nopass`/`check_port_available`/`check_disk_space` 接受 `&dyn CommandRunner`；`preflight_all_nodes` 使用 `Arc<dyn CommandRunner>` |
| `src/cluster/deploy.rs` | `src/install/silent_install.rs` | `generate_install_xml` 调用 | WIRED | L34 `generate_install_xml(&install_config)` 生成 XML；函数已改为 `pub` |
| `src/config/mod.rs` | `src/config/cluster.rs` | `pub mod cluster` | WIRED | 验证方式：代码成功编译，且 `load_cluster_config` 在 deploy 路径可用 |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `src/cluster/mod.rs::run_with_runners` | `config` | `load_cluster_config(&args.config)` → TOML 文件 | 是（TOML 反序列化） | FLOWING |
| `src/cluster/deploy.rs::distribute_configs` | `dmmal_ini` / `dmarch_ini` 等 | `generate_dmmal_ini(all_nodes)` 等函数 | 是（NodeConfig 字段真实传入） | FLOWING |
| `src/cluster/preflight.rs::check_disk_space` | `available` | `parse_df_available(stdout)` 解析远端 df 输出 | 是（解析第 4 列数值） | FLOWING |
| `src/cluster/health.rs::wait_tcp_ready` | TCP 连接结果 | `TcpStream::connect(addr)` | 是（实际 TCP 连接） | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| CLI 解析 cluster deploy --config | `cargo run -- cluster deploy --help` | 输出含 `--config <CONFIG>` | PASS |
| 缺少 --config 时报错 | `cargo run -- cluster deploy` | 报 `required arguments were not provided` | PASS |
| 无效配置（缺 primary）时报错 | `cargo run -- cluster deploy --config cluster_invalid_no_primary.toml` | 报 `必须恰好一个 primary 节点` | PASS |
| 全库测试 | `cargo test` | 80 passed; 0 failed (含 Plan 01/02/03 全部单元测试) | PASS |
| 编译 | `cargo build --bin dm-installer` | exits 0（warnings only） | PASS |
| 无 OpenSSL 依赖 | `cargo tree \| grep openssl` | 无匹配 | PASS |

### Probe Execution

Step 7c: SKIPPED — 无端到端可运行探针文件（端到端验证需真实 SSH 节点，由 Plan 03 Task 4 manual checkpoint 覆盖，已标记 manual-deferred）。

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| CLUS-01 | 03-01, 03-02, 03-03 | 用户可通过 TOML 配置文件部署主备集群，SSH 远程操作，自动生成并分发 dm.ini/dmmal.ini/dmarch.ini/dmwatcher.ini | SATISFIED (代码) / NEEDS HUMAN (运行时) | 完整编排函数链已实现；SFTP 推送 4 个 INI 已有单元测试验证；端到端需真实节点 |
| CLUS-02 | 03-02, 03-03 | 集群部署时，主节点启动并确认健康后再启动备节点 | SATISFIED (代码) / NEEDS HUMAN (运行时) | `run_startup_phase` 强制顺序执行；tracing-test 单元测试验证日志顺序；端到端需真实节点 |
| QUAL-01 | 03-02, 03-03 | 集群部署前执行 SSH 预检查：sudo 免密 / 目标端口可用性 / 磁盘剩余空间 | SATISFIED | `check_sudo_nopass`/`check_port_available`/`check_disk_space` 三项检查均已实现；5 个 mock 单元测试全部通过；预检查失败时中止并报告所有失败节点 |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `src/cluster/mod.rs` | 40 | `type HealthFn` 定义后未使用 | Info | 编译器 dead_code warning；功能不受影响，该类型别名是重构中间态 |
| `src/cluster/ssh.rs` | 204 | `MockRunner` 在非测试代码中 warning | Info | MockRunner 是测试工具，#[cfg(test)] 或 pub 之争；不影响功能 |

无 TBD / FIXME / XXX / unimplemented! 债务标记。

### Human Verification Required

#### 1. 端到端双节点集群部署验证（CLUS-02 SC3 运行时确认）

**Test:** 准备两台测试节点（真实 Linux VM 或 Docker sshd 容器），配置 sudo 免密 + SSH 密钥，准备 DM 安装包；编写 cluster.toml 指向这两台节点；在控制机运行 `RUST_LOG=info cargo run --bin dm-installer -- cluster deploy --config /path/to/cluster.toml 2>&1 | tee /tmp/cluster.log`。

**Expected:** 
1. 日志出现两节点各三项 [预检查] 条目
2. 预检查通过后出现 [node:Primary][1/6] ... [node:Primary][2/6] 序列
3. 出现 "主节点就绪" 字样
4. "主节点就绪" 行之后才出现 "[node:Standby][5/6] 启动达梦备实例" 行（CLUS-02 SC3）
5. 最终输出 "集群部署完成"
6. 两台节点上 `ps -ef | grep dmserver` 和 `ps -ef | grep dmwatcher` 各可见进程

**Why human:** 需要真实 SSH 可达节点和 DM 安装包才能验证完整运行时路径；代码层的有序启动已通过 `#[tracing_test::traced_test]` 单元测试验证，但 CLUS-01 SC1（"两个远程节点均完成达梦安装并建立主备复制关系"）和 CLUS-02 的实际有序启动日志只能通过真实环境观察。

---

### Gaps Summary

无 BLOCKER 级别的代码缺失或 stub。所有 must-have 在代码层面均已 VERIFIED：

- ClusterConfig schema 完整，validate 覆盖 8 条规则
- 4 个 INI 模板生成函数覆盖全部 Pitfall（dmmal 字节相等、dmarch 方向相反、dmwatcher INST_INI 节点专属、OGUID 一致）
- SSH 基础设施（SshError/CommandRunner/SshSession/TofuHandler/MockRunner）完整
- 三项预检查（sudo/端口/磁盘）完整，并发执行，失败报告全部失败节点
- wait_tcp_ready 含 3s 轮询间隔和超时机制
- cluster::run 六步编排完整，预检查失败时不执行任何安装步骤
- CLI `cluster deploy --config` 子命令完整，缺 --config 时拒绝执行
- 80 个测试全部通过，无 FAILED

人工验证项仅涉及端到端运行时行为（真实 SSH + DM 安装包），这是 Plan 03 Task 4 原本就标记为 manual-deferred 的场景。

---

_Verified: 2026-06-13T10:00:00Z_
_Verifier: Claude (gsd-verifier)_
