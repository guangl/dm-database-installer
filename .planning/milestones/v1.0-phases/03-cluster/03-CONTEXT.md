# Phase 3: 主备集群 - Context

**Gathered:** 2026-06-12
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 3 在 Phase 2 的 Rust 二进制基础上，通过 SSH 实现双节点主备集群完整部署：

1. 新增 `cluster deploy --config cluster.toml` 子命令，读取含 `[[cluster.nodes]]` 的 TOML 配置
2. SSH 预检查：sudo 免密权限、目标端口可用性、磁盘剩余空间（QUAL-01）
3. 向各节点 SFTP 推送安装包，并远程执行 DMInstall.bin 静默安装
4. 自动生成并分发 dm.ini / dmmal.ini / dmarch.ini / dmwatcher.ini 到对应节点
5. 有序启动：主节点启动 → TCP 健康确认 → 备节点启动（CLUS-02）

**Phase 2 接管的基础：** `InstallConfig` 结构体、`load_and_validate()`、`config/mod.rs`、tokio async 运行时。
**Phase 4 接管：** 多平台二进制发布流水线，Windows 控制机支持。

</domain>

<decisions>
## Implementation Decisions

### TOML 集群配置 Schema

- **D-01:** 集群节点使用 `[[cluster.nodes]]` 数组表示，每个节点包含 `role`（`"primary"` / `"standby"`）、`host`、`port` 字段。TOML 示例：
  ```toml
  [[cluster.nodes]]
  role = "primary"
  host = "192.168.1.10"
  port = 5236

  [[cluster.nodes]]
  role = "standby"
  host = "192.168.1.11"
  port = 5236
  ```
  数组语法未来可扩展到多备节点（Phase v2 多备），无需改变 schema 结构。

- **D-02:** SSH 凭据以节点级 `[ssh]` 子表表示（内联于每个 `[[cluster.nodes]]` 条目），支持 `user`、`identity_file`（密钥路径）、`password`（可选备用）字段。节点级凭据优先，未来可加顶层 `[cluster.ssh_defaults]` 作公共值。

- **D-03:** `ClusterConfig` 结构体新建于 `config/cluster.rs`，`[[cluster.nodes]]` 反序列化为 `Vec<NodeConfig>`；`ClusterConfig` 与现有 `InstallConfig` 通过顶层 TOML 共存（单机字段可省略）。

### CLI 入口设计

- **D-04:** 新增顶层子命令 `cluster`，下含 `deploy` 子子命令：`dm-installer cluster deploy --config cluster.toml`。与现有 `install` 子命令并列，职责清晰，不改变 Phase 1/2 用户的使用习惯。

- **D-05:** `cluster deploy` 的 `--config` 为必填项（与 `validate` 子命令一致）；不提供时报错，不使用默认配置。

### SSH 认证策略

- **D-06:** 优先使用密钥认证（`identity_file` 字段指定私钥路径）；`password` 字段可选作备用认证。两者均未配置时报错，要求用户至少提供一种凭据。

- **D-07:** 使用 TOFU 策略（Trust On First Use）：首次连接未知主机时自动接受其主机密钥并记录到内存（不持久化到 `~/.ssh/known_hosts`，避免写文件权限问题）；同一 `deploy` 会话内对同一主机复用已接受的密钥。这是 russh `ServerCheckHandler` 自定义实现。

### SSH 预检查（QUAL-01）

- **D-08:** 部署开始前对所有节点并发执行三项预检查（tokio::join_all）：
  1. **sudo 免密**：执行 `sudo -n true`，退出码 0 为通过
  2. **端口可用**：`ss -tlnp | grep :<port>` 无输出为通过（端口未被占用）
  3. **磁盘空间**：`df -B1 <install_path_parent>` 检查剩余空间 ≥ 5 GB

  任一节点任一检查失败 → 终止部署并打印失败节点和检查项，不开始安装。

### 有序启动与健康判据（CLUS-02）

- **D-09:** 主节点安装完成后，控制机通过 TCP 连接测试（`TcpStream::connect` 带超时）轮询主节点端口，默认最多等待 **60 秒**，间隔 **3 秒**，共 20 次。超时后报错终止（不启动备节点）。

- **D-10:** 主节点 TCP 端口可达后，再启动备节点的 DMInstall.bin + dminit 流程。有序启动顺序可从 tracing 日志观察到（CLUS-02 SC3 验收）。

### 配置文件生成与分发（CLUS-01）

- **D-11:** dm.ini / dmmal.ini / dmarch.ini / dmwatcher.ini 在控制机上以模板字符串生成（`format!` 或 `include_str!`），再通过 SFTP 推送到各节点对应目录。不在远程节点上执行文件生成命令，避免依赖远端工具。

- **D-12:** 配置文件模板内容以 `cluster/templates/` 子模块集中管理（Rust 模块内 `const` 或 `include_str!`），便于维护和测试。

### Claude's Discretion

- russh `ClientConfig` 使用 rustle（rustls）TLS backend，与 Phase 2 reqwest 的 `rustls-tls` 策略一致，不引入 C FFI 依赖。
- 多节点并发安装（主备可并发推包），只在"启动"阶段有序（先主后备）；安装/提取阶段可并发加速。
- 错误类型：`ssh` 模块用 `thiserror` 定义 `SshError`（连接失败、命令执行失败、SFTP 失败），顶层用 `anyhow` 包装。
- 日志格式：每步以 `[node:primary][N/M] 步骤名` 前缀，与 Phase 1 的 `[N/7]` 模式一致但加节点标识。

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 需求与路线图

- `.planning/REQUIREMENTS.md` — CLUS-01（SSH 远程操作 + 配置文件分发）、CLUS-02（有序启动）、QUAL-01（SSH 预检查）的完整描述和验收标准
- `.planning/ROADMAP.md` §Phase 3 — 阶段目标、成功标准（4 条）、依赖关系
- `.planning/PROJECT.md` §Constraints — 技术栈约束

### 先前阶段决策

- `.planning/phases/02-toml/02-CONTEXT.md` — Phase 2 的 `InstallConfig` 结构、`load_and_validate()`、CLI 模式；Phase 3 的 `ClusterConfig` 与之共存
- `.planning/phases/01-curl-sh/01-CONTEXT.md` — Phase 1 CLI 结构（D-03 D-04 D-07）、默认安装参数

### 技术参考

- `CLAUDE.md` §Technology Stack — russh 0.61.2 + russh-sftp 2.3.0 版本（SSH/SFTP）、tokio 1.52.3（异步并发）
- `CLAUDE.md` §What NOT to Use — 禁止 ssh2（C FFI）、native-tls、openssl
- `CLAUDE.md` §Stack Patterns by Variant §Cluster Deployment Pattern — tokio::spawn 多节点并发、russh SFTP 推包模式
- `CLAUDE.md` §DM Silent Installation Integration — XMLResponse file 格式、DMInstall.bin -q 用法

### 已有代码（Phase 2 worktree）

- `.claude/worktrees/agent-a693079c0c4cadfbf/src/config/mod.rs` — `InstallConfig` + `load_and_validate()`；Phase 3 `ClusterConfig` 与之并列
- `.claude/worktrees/agent-a693079c0c4cadfbf/src/cli.rs` — `Commands` 枚举；Phase 3 新增 `Cluster(ClusterArgs)` 分支
- `.claude/worktrees/agent-a693079c0c4cadfbf/src/install/mod.rs` — 安装编排器；远程节点安装复用其中的 silent_install / init 逻辑（需抽象为可接受远程执行上下文的函数）

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets

- `config::InstallConfig`（`config/mod.rs`）：单机安装参数结构体，每个集群节点的安装参数可内嵌一个 `InstallConfig` 实例，或直接在 `NodeConfig` 中平铺相同字段
- `config::validate_install_config()`：语义验证纯函数，可在每个 `NodeConfig` 的安装参数上复用
- `install::silent_install` / `install::init`：本地执行的安装步骤，Phase 3 需将其抽象为"接受执行上下文（本地 or SSH）"的版本

### Established Patterns

- 错误处理：顶层 `anyhow::Result<()>`，模块内 `thiserror` 类型化错误
- CLI 参数：clap derive macro，`#[arg(long)]`；子子命令用 `#[command(subcommand)]` 嵌套
- 日志：`tracing::info!` 在每步开始处打进度前缀
- 异步：tokio，所有 I/O 操作 async；multi-node 用 `tokio::join_all` 并发

### Integration Points

- `cli::Commands` 枚举 → 新增 `Cluster(ClusterArgs)` variant，`ClusterArgs` 含 `deploy` 子命令
- `main.rs` match 分支 → 新增 `Commands::Cluster` 路由到 `cluster::run()`
- `config` 模块 → 新增 `cluster.rs`，定义 `ClusterConfig` + `NodeConfig` + `SshCredentials`

</code_context>

<specifics>
## Specific Ideas

- 集群 TOML 示例（控制机使用）：
  ```toml
  [cluster]
  installer_package = "/tmp/dm8_setup.iso"  # 控制机上的安装包路径

  [[cluster.nodes]]
  role = "primary"
  host = "192.168.1.10"
  port = 5236
  install_path = "/opt/dmdbms"
  data_path = "/opt/dmdbms/data"

  [cluster.nodes.ssh]
  user = "root"
  identity_file = "~/.ssh/id_rsa"

  [[cluster.nodes]]
  role = "standby"
  host = "192.168.1.11"
  port = 5236
  install_path = "/opt/dmdbms"
  data_path = "/opt/dmdbms/data"

  [cluster.nodes.ssh]
  user = "root"
  identity_file = "~/.ssh/id_rsa"
  ```

- SSH 预检查输出示例（QUAL-01 验收）：
  ```
  [预检查] 192.168.1.10 (primary)
    ✓ sudo 免密
    ✓ 端口 5236 可用
    ✗ 磁盘空间不足 (剩余 2.1 GB，需要 ≥ 5 GB)
  预检查失败 — 中止部署
  ```

- 有序启动日志示例（CLUS-02 SC3 验收）：
  ```
  [node:primary][5/6] 启动达梦主实例
  [node:primary] 等待主节点健康 (TCP:5236)... 3s/60s
  [node:primary] 主节点就绪 ✓
  [node:standby][5/6] 启动达梦备实例
  ```

</specifics>

<deferred>
## Deferred Ideas

- **多备节点（1 主 N 备）** — `[[cluster.nodes]]` schema 已支持，执行逻辑 Phase v2
- **DSC/DPC 集群** (CLUS-V2-01/03) — v2 需求，拓扑更复杂，单独阶段
- **Windows 控制机支持** — Phase 4 统一处理（russh 跨平台，理论可行）
- **集群清理命令** `cluster clean` (CLUS-V2-02) — v2 需求
- **`--dry-run` 模式** (OPS-V2-01) — v2 需求
- **DOWN-01 自动下载** — 仍为 P2 风险，Phase 3 沿用 `installer_package` 本地路径

</deferred>

---

*Phase: 3-主备集群*
*Context gathered: 2026-06-12*
