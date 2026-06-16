# Phase 6: status 命令 - Context

**Gathered:** 2026-06-14
**Status:** Ready for planning

<domain>
## Phase Boundary

新增 `dm-installer status` 子命令，查询本地 DM 实例及所有 config.toml 中配置的远程节点的进程/端口/角色状态，输出对齐表格。核心交付：

1. **本地状态**：通过 `ps aux` + TCP 端口检测获取本地 DM 进程与端口状态。
2. **远程节点状态**：读取 config.toml（可选），SSH 并发查询所有节点进程/端口/角色。
3. **表格输出**：对齐表格，含 Node | Host | Process | Port | Role 列。

**不在本 phase 范围：**
- 修改现有 SYSDBA 密码硬编码问题（CR-02，留后续处理）
- SSH 端口可配置（CR-01，留后续处理）
- 历史状态记录或监控能力
- Windows 本地进程检测（目标平台为 Linux）

</domain>

<decisions>
## Implementation Decisions

### CLI 结构

- **D-01:** 在 `src/cli.rs` 添加 `Status(StatusArgs)` 变体到 `Commands` 枚举；`StatusArgs` 无必填参数（config.toml 自动发现）。
- **D-02:** 在 `src/main.rs` 添加 `cli::Commands::Status(args) => status::run(args).await` dispatch 分支。
- **D-03:** 新建模块 `src/status/mod.rs`，在 `main.rs` 中声明 `mod status`。

### 配置发现（No-config 行为）

- **D-04:** `dm-installer status` 在当前目录未找到 config.toml 时**不报错**——仅显示本地 DM 实例状态（进程 + 端口），与 `guide.rs` 的无侵入风格一致。
- **D-05:** 找到 config.toml 时，用现有 `config::load_config()` 加载；若为 `LoadedConfig::Cluster`，SSH 查询所有节点；若为 `LoadedConfig::Standalone`，仅显示本地状态（standalone 无远程节点列表）。

### 本地实例检测

- **D-06:** 本地进程检测：`std::process::Command::new("sh").arg("-c").arg("ps aux | grep dmserver | grep -v grep")` — 输出非空则 `running`，否则 `stopped`。
- **D-07:** 本地端口检测：TCP connect 到 `127.0.0.1:PORT`（PORT 来自 config.toml 或默认 5236）；连通则 `listening`，否则 `closed`。
- **D-08:** 本地角色查询：仅当端口 listening 时尝试 `disql SYSDBA/SYSDBA@localhost:PORT "SELECT STATUS\$,MODE\$ FROM V\$INSTANCE;"`；若失败则 Role 显示 `unknown`。

### 远程节点查询

- **D-09:** 使用 `SshSession::connect(host, 22, &node.ssh)` 建立连接，复用现有 SSH 基础设施。
- **D-10:** 远程进程检测：`ps aux | grep dmserver | grep -v grep`（与 D-06 本地方法对称）。
- **D-11:** 远程端口检测：`ss -tlnp | grep ':{PORT}'`（与 `preflight.rs` 现有模式一致）。
- **D-12:** 远程角色查询：仅当端口 listening 时执行 `disql SYSDBA/SYSDBA@localhost:{PORT} "SELECT STATUS\$,MODE\$ FROM V\$INSTANCE;"`；解析输出中 `PRIMARY`/`STANDBY`/`OPEN` 关键词。
- **D-13:** 凭据来源：延用现有代码库的 `SYSDBA/SYSDBA` 硬编码模式（CR-02 是已知问题，Phase 6 不修复）。

### 并发与错误处理

- **D-14:** 所有远程节点并发查询：`tokio::join_all`（UX 更好，与集群部署并发模式一致）。
- **D-15:** 单个节点 SSH 连接失败时：该行 Process/Port/Role 列均显示 `—`，Role 列改为 `ERROR: {原因}`；其余节点正常输出，整体命令退出码 0（非致命错误）。

### 输出格式

- **D-16:** 输出格式：手动对齐文本表格，无额外 crate 依赖。固定列顺序：`Node | Host | Process | Port | Role`。
- **D-17:** 表头与分隔线示例：
  ```
  Node     Host            Process  Port  Role
  -------  --------------  -------  ----  -------
  local    localhost       running  open  PRIMARY
  standby  192.168.1.101   running  open  STANDBY
  node2    192.168.1.102   stopped  —     —
  ```
- **D-18:** 节点名称（Node 列）来源：cluster config 的节点 `role` 字段（Primary/Standby）；本地节点固定显示 `local`。

### Claude's Discretion

- 角色字符串解析逻辑（从 disql 输出中提取 STATUS$/MODE$ 值的具体正则/字符串匹配）
- 表格列宽动态计算还是固定宽度（根据最长主机名动态调整更美观）
- 超时参数：SSH 连接超时、disql 查询超时（建议各 5s）

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### CLI 与入口

- `src/cli.rs` — 现有 Commands 枚举和 Args 模式；新增 Status 变体须遵循此风格
- `src/main.rs` — dispatch 模式；新增 Status 分支须在此添加

### SSH 基础设施

- `src/common/ssh/session.rs` — `SshSession::connect(host, port, creds)`，SSH 连接入口
- `src/common/ssh/runner.rs` — `CommandRunner` trait：`exec(cmd) -> Result<(Vec<u8>, u32)>`
- `src/config/ssh.rs` — `SshCredentials` 结构（user, identity_file, password）

### 配置加载

- `src/config/mod.rs` 或 `src/config/cluster.rs` — `load_config()` 返回 `LoadedConfig`；`NodeConfig` 结构（host, role, ssh: SshCredentials）
- `src/config/cluster.rs:190` — `NodeConfig` 定义，含 `host`, `role`, `ssh` 字段

### 现有检测模式

- `src/cluster/preflight.rs:25` — `ss -tlnp | grep ':{port}'` 端口检测模式
- `src/cluster/phases.rs` — `disql SYSDBA/SYSDBA@localhost:{port}` SQL 查询模式（角色查询参考）
- `src/cluster/health.rs` — TCP connect 健康检查模式

### 需求与验收标准

- `.planning/REQUIREMENTS.md` STAT-01, STAT-02, STAT-03
- `.planning/ROADMAP.md` Phase 6 Success Criteria（4条）

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets

- `SshSession::connect` + `CommandRunner::exec` — 完整 SSH 命令执行链，status 查询直接复用
- `config::load_config()` — 现有配置加载，返回 `LoadedConfig`（Standalone/Cluster）
- `preflight.rs` 的 `ss -tlnp` 模式 — 远程端口检测可直接复用命令字符串
- `health.rs::wait_tcp_ready` — 本地端口检测的 TCP connect 思路（status 用一次性检测而非轮询）

### Established Patterns

- **CommandRunner trait**：所有远程命令执行通过 `runner.exec(&cmd).await` 返回 `(stdout_bytes, exit_code)`
- **并发模式**：集群代码多处用 `tokio::join_all` / `try_join_all` 并发查询多节点
- **函数命名**：`run_xxx` 用于 phase 执行；status 模块顶层入口建议命名 `pub async fn run(args: &StatusArgs) -> Result<()>`

### Integration Points

- `src/main.rs` — 新增 `mod status;` 和 `Commands::Status(args) => status::run(args).await`
- `src/cli.rs` — 新增 `Status(StatusArgs)` 到 `Commands` 枚举
- `src/status/mod.rs` — 新文件，实现整个 status 功能

</code_context>

<deferred>
## Deferred Ideas

- 修复 SSH 端口硬编码（CR-01）——NodeConfig 需增加 ssh_port 字段，留 Phase 7 或专项 fix
- 修复 SYSDBA 密码硬编码（CR-02）——config.toml 增加 db_password 字段，留后续 phase
- `--watch` 模式（持续刷新状态表格）——超出本 phase 范围
- JSON 输出格式（`--format json`）——可选增强，本 phase 不做
- Windows 本地进程检测——需 `tasklist | findstr dmserver`，留多平台扩展时处理

</deferred>

---

*Phase: 6 — status 命令*
*Context gathered: 2026-06-14*
