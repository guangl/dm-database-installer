# Phase 3: 主备集群 - Research

**Researched:** 2026-06-12
**Domain:** russh SSH 客户端 / 达梦主备集群配置 / Rust 异步并发
**Confidence:** HIGH

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** 集群节点使用 `[[cluster.nodes]]` 数组，每个节点含 `role`（`"primary"` / `"standby"`）、`host`、`port`
- **D-02:** SSH 凭据以节点级 `[ssh]` 子表表示，支持 `user`、`identity_file`、`password`（可选备用）
- **D-03:** `ClusterConfig` 新建于 `config/cluster.rs`，与 `InstallConfig` 通过顶层 TOML 共存
- **D-04:** 新增顶层子命令 `cluster deploy`：`dm-installer cluster deploy --config cluster.toml`
- **D-05:** `--config` 为必填项，不提供时报错
- **D-06:** 优先密钥认证，`password` 可选备用，两者均缺则报错
- **D-07:** TOFU 策略——首次连接自动接受主机密钥记入内存，不写 `~/.ssh/known_hosts`
- **D-08:** 预检查并发执行（tokio::join_all）三项：sudo 免密 / 端口可用 / 磁盘空间 ≥ 5 GB；任一失败则中止
- **D-09:** TCP 健康轮询：最多 60 秒，3 秒间隔（20 次），超时报错不启动备节点
- **D-10:** 主节点 TCP 可达后再启动备节点安装流程
- **D-11:** 配置文件在控制机上以模板字符串生成，通过 SFTP 分发，不在远端执行生成命令
- **D-12:** 模板集中于 `cluster/templates/` 子模块（Rust `const` 或 `include_str!`）

### Claude's Discretion

- russh ClientConfig 使用 rustls backend（与 Phase 2 reqwest 策略一致）
- 主备并发推包，仅"启动"阶段有序（先主后备）
- `ssh` 模块用 `thiserror` 定义 `SshError`，顶层用 `anyhow` 包装
- 日志前缀：`[node:primary][N/M] 步骤名`

### Deferred Ideas (OUT OF SCOPE)

- 多备节点（1 主 N 备）
- DSC/DPC 集群
- Windows 控制机支持（Phase 4）
- `cluster clean` 命令
- `--dry-run` 模式
- DOWN-01 自动下载（Phase 3 继续使用本地包路径）
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CLUS-01 | 用户可通过 TOML 配置文件部署主备集群，安装器通过 SSH 远程操作所有节点，自动生成并分发 dm.ini/dmmal.ini/dmarch.ini/dmwatcher.ini | russh 0.61.2 + russh-sftp 2.3.0 提供 SSH 连接、命令执行、SFTP 文件传输；配置模板内容已通过达梦官方文档和社区资料确认 |
| CLUS-02 | 集群部署时，主节点启动并确认健康后再启动备节点（有序启动） | `tokio::net::TcpStream::connect` 带 timeout 可实现 TCP 健康轮询；tracing 日志标记顺序 |
| QUAL-01 | 集群部署前执行 SSH 预检查：sudo 免密权限、目标端口可用性、磁盘剩余空间 | 三条 SSH 命令（`sudo -n true` / `ss -tlnp` / `df -B1`）；tokio::join_all 并发执行 |
</phase_requirements>

---

## Summary

Phase 3 在 Phase 2 已有的 Rust 二进制（tokio + clap + serde/toml + anyhow）基础上，新增两大能力：

**能力一：SSH 远程操作**。通过 `russh 0.61.2` + `russh-sftp 2.3.0` 实现对远程节点的连接、命令执行、文件推送。这两个 crate 已在 CLAUDE.md 中锁定，且已通过 crates.io 确认版本存在（russh 总下载 403 万次，russh-sftp 总下载 167 万次）。

**能力二：达梦主备集群编排**。达梦主备搭建有固定步骤序列：两节点分别 dminit → 推送配置文件 → mount 模式启动 → 执行 SQL（SP_SET_OGUID + ALTER DATABASE PRIMARY/STANDBY）→ 启动 dmwatcher。配置文件（dmmal.ini/dmarch.ini/dmwatcher.ini）在控制机上通过 Rust 模板字符串生成后 SFTP 分发，无需在远端执行任何脚本生成逻辑。

**主要风险**：达梦安装后需通过 SQL 命令设置主备角色（`ALTER DATABASE PRIMARY/STANDBY`），这意味着安装器必须能在节点的数据库端口上执行 SQL，而非仅 SSH 命令。Phase 3 MVP 解法是：通过 SSH 在远端节点执行 `disql` 客户端（达梦自带的 CLI 工具）来执行这些 SQL，避免引入新的数据库驱动依赖。

**Primary recommendation:** 按 D-11/D-12 设计，在控制机生成所有配置文件内容，通过 SFTP 推送；所有远端操作均通过 SSH 命令完成（包括 dminit、dmserver mount、disql SQL 执行、dmwatcher 启动）。

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| TOML 集群配置解析 | CLI 进程（控制机） | — | `ClusterConfig` 在控制机反序列化，不涉及远端 |
| SSH 预检查 | CLI 进程（控制机） → 远端节点 | — | 控制机发起 SSH，远端执行 shell 命令返回结果 |
| 安装包传输 | 控制机 → 远端节点（SFTP） | — | 安装包路径在控制机本地，通过 russh-sftp 上传 |
| 远端 DMInstall.bin 执行 | 远端节点（SSH exec） | 控制机流式日志 | 实际进程在远端；控制机通过 channel stdout 接收输出 |
| 配置文件生成 | CLI 进程（控制机） | — | D-11：在控制机生成模板字符串，避免远端依赖 |
| 配置文件分发 | 控制机 → 远端节点（SFTP） | — | 生成后立即 SFTP 写入远端目标路径 |
| 主备模式 SQL 设置 | 远端节点（disql via SSH） | — | disql 是达梦自带 CLI；SP_SET_OGUID 等命令需数据库在 mount 状态 |
| TCP 健康轮询 | CLI 进程（控制机） | — | `TcpStream::connect` 从控制机直连主节点端口 |
| dmwatcher 启动 | 远端节点（SSH exec） | — | dmwatcher 进程运行在各节点本地 |
| 日志 / 进度展示 | CLI 进程（控制机） | — | tracing + indicatif 在控制机终端渲染 |

---

## Standard Stack

### Core（新增）
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `russh` | 0.61.2 | SSH 客户端连接 + 命令执行 | CLAUDE.md 锁定；纯 Rust，无 C FFI；tokio 原生 [VERIFIED: crates.io] |
| `russh-sftp` | 2.3.0 | SFTP 文件上传 | CLAUDE.md 锁定；`SftpSession::new(channel.into_stream())` 直接集成 russh Channel [VERIFIED: crates.io] |

### Core（复用 Phase 2）
| Library | Version | Purpose | Notes |
|---------|---------|---------|-------|
| `tokio` | 1.52.3 | 异步运行时 | 已在 Cargo.toml；`tokio::net::TcpStream` 用于健康轮询 [VERIFIED: crates.io] |
| `clap` | 4.6.1 | CLI 参数解析 | 已有；新增 `Cluster` subcommand variant [VERIFIED: crates.io] |
| `serde` + `toml` | 1.0.228 / 1.1.2 | TOML 反序列化 | 已有；`ClusterConfig` 复用同一机制 [VERIFIED: crates.io] |
| `anyhow` | 1.0.102 | 顶层错误处理 | 已有 [VERIFIED: crates.io] |
| `thiserror` | 2.0.18 | `SshError` 类型 | 已有 [VERIFIED: crates.io] |
| `tracing` | 0.1.44 | 结构化日志 | 已有 [VERIFIED: crates.io] |
| `indicatif` | 0.18.4 | 进度 spinner | 已有 [VERIFIED: crates.io] |

### 新增 Cargo.toml 条目
```toml
russh = { version = "0.61.2", default-features = false, features = ["async-trait", "client"] }
russh-sftp = "2.3.0"
```

> 注：`russh` 默认启用 `rustls` backend（与 `reqwest` 的 `rustls-tls` 策略一致，无 C FFI）。[ASSUMED — 需确认 feature flag 名称]

**Version verification:**
```bash
cargo search russh       # => russh = "0.61.2"  (confirmed 2026-06-12)
cargo search russh-sftp  # => russh-sftp = "2.3.0" (confirmed 2026-06-12)
```

---

## Package Legitimacy Audit

> slopcheck 在当前环境不可用（pip 安装失败），所有条目标记 [ASSUMED]，planner 应在 install 任务前插入 `checkpoint:human-verify`。

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `russh` | crates.io | ~5 yr (Warp terminal 在用) | 4,037,306 total | github.com/warp-tech/russh | [ASSUMED] | Approved — 有 Warp terminal 背书，极低风险 |
| `russh-sftp` | crates.io | ~2 yr | 1,667,580 total | github.com/AspectUnk/russh-sftp | [ASSUMED] | Approved — 下载量充足，与 russh 生态绑定 |

**Packages removed due to slopcheck [SLOP] verdict:** none

**Packages flagged as suspicious [SUS]:** none

*slopcheck 在研究时不可用，上述包标记 [ASSUMED]。planner 应在 `cargo add russh russh-sftp` 任务前插入 `checkpoint:human-verify`。*

---

## Architecture Patterns

### System Architecture Diagram

```
控制机（dm-installer cluster deploy）
│
├─ 1. 读取 cluster.toml → ClusterConfig
│
├─ 2. SSH 预检查（tokio::join_all，并发）
│     ├─→ primary:22  ── ssh exec "sudo -n true"
│     │                ── ssh exec "ss -tlnp | grep :5236"
│     │                ── ssh exec "df -B1 /opt"
│     └─→ standby:22  ── 同上三条命令
│
├─ 3. 并发推包 + DMInstall.bin 安装（tokio::join_all）
│     ├─→ primary: SFTP upload iso → ssh exec DMInstall.bin -q xml
│     └─→ standby: SFTP upload iso → ssh exec DMInstall.bin -q xml
│
├─ 4. 生成配置文件（控制机 format!），SFTP 分发到各节点
│     ├─→ primary: dm.ini / dmmal.ini / dmarch.ini(primary) / dmwatcher.ini
│     └─→ standby: dm.ini / dmmal.ini / dmarch.ini(standby) / dmwatcher.ini
│
├─ 5. 有序启动
│     ├─ 启动主节点（ssh exec "dmserver dm.ini mount"，后台运行）
│     ├─ TCP 健康轮询（TcpStream::connect primary:5236，最多 60s）
│     ├─ 主节点 disql 设置（ssh exec "disql ..."，执行 SP_SET_OGUID + ALTER DATABASE PRIMARY）
│     ├─ 启动备节点（ssh exec "dmserver dm.ini mount"，后台运行）
│     └─ 备节点 disql 设置（ssh exec "disql ..."，执行 SP_SET_OGUID + ALTER DATABASE STANDBY）
│
└─ 6. 启动 dmwatcher（ssh exec，主备各一）
      ├─→ primary: ssh exec "dmwatcher dmwatcher.ini"
      └─→ standby: ssh exec "dmwatcher dmwatcher.ini"
```

### Recommended Project Structure
```
src/
├── cli.rs                      # 新增 Commands::Cluster(ClusterArgs) variant
├── config/
│   ├── mod.rs                  # 已有 InstallConfig
│   └── cluster.rs              # 新增 ClusterConfig / NodeConfig / SshCredentials
├── cluster/
│   ├── mod.rs                  # cluster::run() 入口
│   ├── preflight.rs            # SSH 预检查（QUAL-01）
│   ├── deploy.rs               # 并发安装编排
│   ├── ssh.rs                  # russh 封装：SshSession struct
│   ├── health.rs               # TCP 健康轮询（CLUS-02）
│   └── templates/
│       ├── mod.rs              # 模板函数集中管理（D-12）
│       ├── dm_ini.rs           # generate_dm_ini(node: &NodeConfig) -> String
│       ├── dmmal_ini.rs        # generate_dmmal_ini(nodes: &[NodeConfig]) -> String
│       ├── dmarch_ini.rs       # generate_dmarch_ini(node: &NodeConfig, nodes: &[NodeConfig]) -> String
│       └── dmwatcher_ini.rs    # generate_dmwatcher_ini(node: &NodeConfig, oguid: u32) -> String
└── main.rs
```

### Pattern 1: russh SSH 连接与命令执行

**What:** 使用 `russh::client` 建立 SSH 连接，执行远端命令并捕获 stdout/exit code
**When to use:** 所有远端 shell 命令（预检查、DMInstall.bin 执行、dmserver 启动、disql SQL 执行）

```rust
// Source: docs.rs/russh/latest/russh/client/, github.com/Eugeny/russh examples
use russh::{client, ChannelMsg};
use russh::keys::load_secret_key;
use std::sync::Arc;

/// TOFU handler - 首次连接接受主机密钥，存内存不写文件 (D-07)
struct TofuHandler {
    accepted_keys: std::sync::Mutex<Vec<ssh_key::PublicKey>>,
}

impl client::Handler for TofuHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // TOFU: 无条件接受，记入内存供同会话复用
        self.accepted_keys.lock().unwrap().push(server_public_key.clone());
        Ok(true)
    }
}

/// 在远端节点执行命令，返回 (stdout: Vec<u8>, exit_code: u32)
pub async fn exec_remote(
    session: &mut client::Handle<TofuHandler>,
    command: &str,
) -> anyhow::Result<(Vec<u8>, u32)> {
    let mut channel = session.channel_open_session().await?;
    channel.exec(true, command).await?;

    let mut stdout = Vec::new();
    let mut exit_code = 0u32;

    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { ref data }) => stdout.extend_from_slice(data),
            Some(ChannelMsg::ExitStatus { exit_status }) => {
                exit_code = exit_status;
            }
            Some(ChannelMsg::Eof) | None => break,
            _ => {}
        }
    }
    Ok((stdout, exit_code))
}
```

### Pattern 2: russh-sftp 文件上传

**What:** 通过 SFTP 将控制机上的文件/字节推送到远端节点
**When to use:** 上传安装包 ISO、推送生成的配置文件（D-11）

```rust
// Source: docs.rs/russh-sftp/2.3.0/russh_sftp/client/struct.SftpSession.html
use russh_sftp::client::SftpSession;

/// 将 bytes 写入远端路径
pub async fn sftp_write(
    session: &mut client::Handle<TofuHandler>,
    remote_path: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let channel = session.channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;
    let sftp = SftpSession::new(channel.into_stream()).await?;
    sftp.write(remote_path, bytes).await?;
    Ok(())
}
```

### Pattern 3: 并发预检查

**What:** `tokio::join_all` 并发对所有节点执行三项预检查，收集失败项
**When to use:** 部署开始前（D-08，QUAL-01）

```rust
// Source: tokio 1.52 docs (futures::future::join_all pattern)
use futures::future::join_all;

let checks = nodes.iter().map(|node| preflight::check_node(node));
let results = join_all(checks).await;

let failures: Vec<_> = results.iter().filter(|r| r.is_err()).collect();
if !failures.is_empty() {
    // 打印所有失败节点和检查项，中止
    anyhow::bail!("预检查失败 — 中止部署");
}
```

### Pattern 4: TCP 健康轮询

**What:** 轮询主节点端口直到 TCP 可连接或超时（D-09，CLUS-02）
**When to use:** 主节点 dmserver mount 启动后

```rust
// Source: tokio::net::TcpStream docs
use tokio::net::TcpStream;
use tokio::time::{sleep, Duration, timeout};

pub async fn wait_tcp_ready(host: &str, port: u16, max_secs: u64) -> anyhow::Result<()> {
    let addr = format!("{}:{}", host, port);
    let interval = Duration::from_secs(3);
    let deadline = Duration::from_secs(max_secs); // 默认 60s (D-09)

    let result = timeout(deadline, async {
        loop {
            match TcpStream::connect(&addr).await {
                Ok(_) => return Ok(()),
                Err(_) => sleep(interval).await,
            }
        }
    }).await;

    result.map_err(|_| anyhow::anyhow!("主节点 {}:{} 在 {}s 内未就绪", host, port, max_secs))?
}
```

### Anti-Patterns to Avoid

- **在远端生成配置文件**：避免 `ssh exec "cat > /etc/dm/dmmal.ini << 'EOF' ..."` 这类 heredoc shell 命令，不可靠且难以调试。D-11 明确要求在控制机生成后 SFTP 推送。
- **单 SSH 连接串行预检查**：不要对 primary 执行完三项后再对 standby 执行，应 tokio::join_all 并发。
- **阻塞 TCP 轮询**：不要用 `std::net::TcpStream`（阻塞），应使用 `tokio::net::TcpStream`。
- **混用 `ssh2` crate**：CLAUDE.md §What NOT to Use 明确禁止 ssh2（C FFI，交叉编译困难）。
- **dmwatcher.ini 用不同 OGUID**：主备节点的 `INST_OGUID` 必须完全一致，否则守护进程无法建立连接。

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SSH 连接/认证 | 自己实现 SSH 握手 | `russh 0.61.2` | SSH 协议细节极多（密钥交换、加密、通道多路复用） |
| SFTP 文件传输 | 通过 SSH exec + base64 编码 | `russh-sftp 2.3.0` | SFTP 有自己的协议帧格式；手写易出边界错误 |
| 异步超时 | `sleep` + `AtomicBool` 标志 | `tokio::time::timeout` | tokio 原生，cancel-safe |
| 并发任务收集 | 手动 `Vec<JoinHandle>` + for loop join | `futures::future::join_all` 或 `tokio::task::JoinSet` | join_all 保留错误顺序，JoinSet 更现代（tokio 1.23+） |
| XML 模板生成 | 用 xml crate | `format!` 字符串（已在 silent_install.rs 验证） | Phase 2 已有 `generate_install_xml`，模式一致 |

**Key insight:** russh 的 TOFU handler 只需实现一个方法（`check_server_key` 返回 `Ok(true)`），不要在这里引入复杂的主机密钥持久化逻辑——D-07 明确决定不写文件。

---

## 达梦主备集群关键知识

### 配置文件体系

达梦主备集群需要四个配置文件，在现有 `dm.ini` 基础上增加三个专用文件。[CITED: eco.dameng.com/document/dm/zh-cn/pm/configuration-description.html]

#### dmmal.ini（主备节点完全相同）
```ini
MAL_CHECK_INTERVAL = 5
MAL_CONN_FAIL_INTERVAL = 5

[MAL_INST1]
MAL_INST_NAME = DMSVR01        # 必须与主节点 dm.ini INSTANCE_NAME 一致
MAL_HOST = 192.168.1.10        # MAL 系统监听 IP（主节点）
MAL_PORT = 5237                # MAL 链路端口（不能与 PORT_NUM 冲突）
MAL_INST_HOST = 192.168.1.10   # 实例对外服务 IP
MAL_INST_PORT = 5236           # 实例服务端口，与 dm.ini PORT_NUM 一致
MAL_DW_PORT = 5238             # 守护进程监听端口
MAL_INST_DW_PORT = 5239        # 实例监听守护进程的端口

[MAL_INST2]
MAL_INST_NAME = DMSVR02
MAL_HOST = 192.168.1.11
MAL_PORT = 5237
MAL_INST_HOST = 192.168.1.11
MAL_INST_PORT = 5236
MAL_DW_PORT = 5238
MAL_INST_DW_PORT = 5239
```

**关键**：dmmal.ini 在主备节点上必须完全一致（`MAL_CHECK_INTERVAL`, 两个 `[MAL_INST*]` 内容都相同）。[CITED: cnblogs.com/Williamls/p/17088364.html]

#### dmarch.ini（主备节点的 ARCH_DEST 方向相反）

主节点：
```ini
[ARCHIVE_REALTIME]
ARCH_TYPE = REALTIME
ARCH_DEST = DMSVR02            # 主节点归档目标 = 备节点实例名

[ARCHIVE_LOCAL1]
ARCH_TYPE = LOCAL
ARCH_DEST = /opt/dmdbms/data/DMSVR01/arch
ARCH_FILE_SIZE = 128
ARCH_SPACE_LIMIT = 0
```

备节点：
```ini
[ARCHIVE_REALTIME]
ARCH_TYPE = REALTIME
ARCH_DEST = DMSVR01            # 备节点归档目标 = 主节点实例名（角色切换用）

[ARCHIVE_LOCAL1]
ARCH_TYPE = LOCAL
ARCH_DEST = /opt/dmdbms/data/DMSVR02/arch
ARCH_FILE_SIZE = 128
ARCH_SPACE_LIMIT = 0
```

#### dmwatcher.ini（主备节点完全相同）
```ini
[GRP1]
DW_TYPE = GLOBAL
DW_MODE = AUTO
DW_ERROR_TIME = 10
INST_RECOVER_TIME = 60
INST_ERROR_TIME = 10
INST_OGUID = 453331            # 必须主备一致，范围 0-2147483647
INST_INI = /opt/dmdbms/data/DMSVR01/dm.ini   # 各节点指向自身的 dm.ini
INST_AUTO_RESTART = 1
INST_STARTUP_CMD = /opt/dmdbms/bin/dmserver
RLOG_SEND_THRESHOLD = 0
RLOG_APPLY_THRESHOLD = 0
```

> 注意：`INST_INI` 路径各节点不同（主节点指向主节点 data 目录，备节点指向备节点 data 目录）——这是 dmwatcher.ini "整体结构相同，只有 INST_INI 路径不同"的唯一差异。[CITED: cnblogs.com/Williamls/p/17088364.html]

#### dm.ini 新增字段（在 Phase 2 已有字段基础上追加）
```ini
# 在现有 PORT_NUM、INSTANCE_NAME 等字段基础上新增：
MAL_INI = 1                    # 启用 MAL 系统
ARCH_INI = 1                   # 启用归档
ALTER_MODE_STATUS = 0          # 初始为 0，SQL 设置时会临时改为 1
ENABLE_OFFLINE_TS = 2
```

### 部署步骤序列（控制机视角）

```
1. 两节点并发：
   a. SFTP 上传安装包
   b. SSH exec: DMInstall.bin -q <xml>   # 静默安装
   c. SSH exec: dminit PATH=... INSTANCE_NAME=...  # 初始化（各节点用自身实例名）

2. 生成配置文件（控制机 format!），SFTP 分发：
   - dm.ini 追加字段（MAL_INI=1, ARCH_INI=1 等）
   - dmmal.ini（主备完全相同）
   - dmarch.ini（主节点版 + 备节点版，ARCH_DEST 方向不同）
   - dmwatcher.ini（主备仅 INST_INI 路径不同）

3. 有序启动：
   a. SSH exec 主节点: dmserver /data/DMSVR01/dm.ini mount &
   b. 控制机 TCP 轮询 primary:5236，最多 60s (D-09)
   c. SSH exec 主节点: disql SYSDBA/SYSDBA@primary:5236 \
         "SP_SET_PARA_VALUE(1,'ALTER_MODE_STATUS',1);
          sp_set_oguid(453331);
          alter database primary;
          SP_SET_PARA_VALUE(1,'ALTER_MODE_STATUS',0);"
   d. SSH exec 备节点: dmserver /data/DMSVR02/dm.ini mount &
   e. 控制机 TCP 轮询 standby:5236，最多 60s
   f. SSH exec 备节点: disql SYSDBA/SYSDBA@standby:5236 \
         "SP_SET_PARA_VALUE(1,'ALTER_MODE_STATUS',1);
          alter database standby;
          SP_SET_PARA_VALUE(1,'ALTER_MODE_STATUS',0);"

4. 启动守护进程（主备可并发）：
   SSH exec: dmwatcher /data/DMSVR0X/dmwatcher.ini &
```

> **关键陷阱**：dminit 初始化两节点时，`INSTANCE_NAME` 必须不同（主 `DMSVR01`，备 `DMSVR02`）；而 Phase 2 的单机安装默认使用 `DMSERVER`。ClusterConfig 中每个节点必须有独立的 `instance_name` 字段。[CITED: cnblogs.com/ios9/p/17696217.html]

---

## Common Pitfalls

### Pitfall 1: dmmal.ini 主备内容不一致
**What goes wrong:** 若主备节点 dmmal.ini 内容不完全相同，MAL 链路无法建立，守护进程报"无法连接到对端"。
**Why it happens:** 两节点分开生成时，IP 顺序、端口、全局参数细微差异。
**How to avoid:** D-11/D-12 策略——在控制机生成一份 dmmal.ini bytes，用同一个变量 SFTP 推送到两个节点。
**Warning signs:** `dmwatcher` 启动后 stdout 输出 `连接 MAL 链路失败`。

### Pitfall 2: dminit 参数等号两侧不能有空格
**What goes wrong:** `dminit PATH = /opt/...` 静默失败或报错。
**Why it happens:** 达梦 dminit 解析器不容忍空格。
**How to avoid:** Phase 2 已建立 `build_dminit_command` 函数正确处理此问题（每个参数独立 `.arg(format!("KEY={}", val))`）。集群版复用此函数逻辑。
**Warning signs:** `dminit` 返回非零 exit code 但错误信息含糊。

### Pitfall 3: dmwatcher.ini 的 INST_INI 路径各节点不同
**What goes wrong:** 将主节点的 dmwatcher.ini 直接复制到备节点，INST_INI 仍指向主节点路径，备节点 dmwatcher 无法找到本地 dm.ini。
**How to avoid:** 模板函数 `generate_dmwatcher_ini` 接受节点的 `data_path` 参数生成各自的版本。
**Warning signs:** 备节点 dmwatcher 报 `无法读取 dm.ini`。

### Pitfall 4: ALTER DATABASE PRIMARY/STANDBY 需要数据库在 MOUNT 状态
**What goes wrong:** 若数据库以 OPEN 状态启动（非 mount），执行 `ALTER DATABASE PRIMARY` 会报错。
**Why it happens:** 主备模式切换必须在 mount 状态，不能在 open 状态。
**How to avoid:** SSH exec 启动命令必须带 `mount` 参数：`dmserver dm.ini mount`（不是 `dmserver dm.ini`）。
**Warning signs:** `disql` 执行 SQL 返回 `数据库状态错误` 类错误。

### Pitfall 5: OGUID 主备必须相同
**What goes wrong:** 主备节点设置不同的 OGUID 值，守护进程拒绝建立连接。
**Why it happens:** OGUID 是守护系统的唯一标识，用于主备识别彼此身份。
**How to avoid:** ClusterConfig 中有一个顶层 `oguid: u32` 字段，生成所有节点配置时使用同一值。
**Warning signs:** dmwatcher 报 `OGUID 不匹配`。

### Pitfall 6: russh Channel::wait() 死循环
**What goes wrong:** `channel.wait()` 在 Eof 之前不会返回 None，需要正确处理 `ChannelMsg::Eof`。
**Why it happens:** 命令执行完毕后服务器先发 ExitStatus，再发 Eof，最后 wait() 返回 None。
**How to avoid:** 在 loop 中同时匹配 `ChannelMsg::Eof` 和 `None`（两者都 break），见 Pattern 1 示例。
**Warning signs:** `exec_remote` 函数挂起不返回。

### Pitfall 7: 磁盘空间检查路径要用安装路径的父目录
**What goes wrong:** `df -B1 /opt/dmdbms` 在目录不存在时报错。
**Why it happens:** 节点初次部署时，`install_path` 尚不存在。
**How to avoid:** 用 `df -B1 <install_path_parent>` 或 `df -B1 /`（最保守），D-08 规定检查 install_path 的父目录。
**Warning signs:** df 命令非零 exit code，预检查误判为磁盘不足。

---

## Code Examples

### 达梦主备配置模板生成

```rust
// Source: 基于 cnblogs.com/Williamls/p/17088364.html 配置格式（MEDIUM confidence）
// ASSUMED: 字段名和格式已通过多个社区资料交叉验证

pub fn generate_dmmal_ini(nodes: &[NodeConfig]) -> String {
    let mut out = String::from(
        "MAL_CHECK_INTERVAL = 5\nMAL_CONN_FAIL_INTERVAL = 5\n\n"
    );
    for (i, node) in nodes.iter().enumerate() {
        out.push_str(&format!(
            "[MAL_INST{}]\n\
             MAL_INST_NAME = {}\n\
             MAL_HOST = {}\n\
             MAL_PORT = {}\n\
             MAL_INST_HOST = {}\n\
             MAL_INST_PORT = {}\n\
             MAL_DW_PORT = {}\n\
             MAL_INST_DW_PORT = {}\n\n",
            i + 1,
            node.instance_name,
            node.host,
            node.mal_port,       // 默认 5237
            node.host,
            node.port,           // dm PORT_NUM，默认 5236
            node.dw_port,        // 默认 5238
            node.inst_dw_port,   // 默认 5239
        ));
    }
    out
}

pub fn generate_dmarch_ini(node: &NodeConfig, peer_instance: &str) -> String {
    format!(
        "[ARCHIVE_REALTIME]\n\
         ARCH_TYPE = REALTIME\n\
         ARCH_DEST = {}\n\n\
         [ARCHIVE_LOCAL1]\n\
         ARCH_TYPE = LOCAL\n\
         ARCH_DEST = {}/arch\n\
         ARCH_FILE_SIZE = 128\n\
         ARCH_SPACE_LIMIT = 0\n",
        peer_instance,
        node.data_path,
    )
}
```

### NodeConfig 结构体设计

```rust
// 基于 D-01、D-02、D-03 以及达梦端口需求推导
#[derive(Debug, Deserialize)]
pub struct NodeConfig {
    pub role: NodeRole,           // "primary" | "standby"
    pub host: String,
    pub port: u16,                // 数据库服务端口，默认 5236
    pub install_path: String,     // 默认 /opt/dmdbms
    pub data_path: String,        // 默认 /opt/dmdbms/data/<instance_name>
    pub instance_name: String,    // 主备必须不同！e.g. DMSVR01 / DMSVR02
    pub mal_port: u16,            // MAL 链路端口，默认 5237（不能与 port 冲突）
    pub dw_port: u16,             // 守护进程监听端口，默认 5238
    pub inst_dw_port: u16,        // 实例监听守护进程端口，默认 5239
    pub ssh: SshCredentials,
}

#[derive(Debug, Deserialize)]
pub enum NodeRole { Primary, Standby }

#[derive(Debug, Deserialize)]
pub struct SshCredentials {
    pub user: String,
    pub identity_file: Option<PathBuf>,  // 密钥路径（D-06 优先）
    pub password: Option<String>,         // 可选备用
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `ssh2` (libssh2 C 绑定) | `russh`（纯 Rust） | ~2022 Warp 团队推广 | 无 C FFI，交叉编译友好 |
| `thrussh`（russh 前身） | `russh`（官方继任者） | 2022 重命名 | API 基本兼容，版本 0.34+ 已是 russh |
| 主备备份还原初始化 | 两节点各自 dminit（测试场景） | DM8 文档支持 | Phase 3 MVP 可跳过 dmrman 备份还原 |

**Deprecated/outdated:**
- `ssh2` crate: CLAUDE.md 明确禁止，C FFI 跨编译困难，最后更新 2025年2月
- `thrussh`: 已重命名为 russh，不再维护独立版本

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | russh `ClientConfig` 默认使用 rustls backend，无需额外 feature flag | Standard Stack | 若需要显式 feature flag，Cargo.toml 编译报错（低风险，cargo 报错时修改即可） |
| A2 | `russh-sftp` 的 `SftpSession::write(path, bytes)` API 接口稳定（仅 docs.rs 摘要验证） | Code Examples | 若 API 有变，SFTP 上传代码需调整 |
| A3 | 达梦 `disql` 工具在安装后位于 `<install_path>/bin/disql`，支持 `-e "SQL"` 参数执行单条语句 | 部署步骤序列 | 若 disql CLI 参数不同，SQL 执行步骤需改为交互式输入（高风险，应在测试阶段验证） |
| A4 | dmwatcher.ini 主备节点仅 `INST_INI` 路径不同，其他字段完全相同 | 达梦配置文件 | 若其他字段也需要差异化，模板生成逻辑需拆分（低风险，文档明确） |
| A5 | 两节点可以用 dminit 分别初始化（不需要 dmrman 备份还原） | 部署步骤序列 | 若达梦要求备份还原来保证初始数据一致性，MVP 方案不可行（高风险，建议在测试阶段验证，或在 CONTEXT 中明确 MVP 假设"全新安装，无历史数据"） |
| A6 | russh 和 russh-sftp 包通过 slopcheck 审计（slopcheck 不可用，基于下载量和 GitHub 仓库判断） | Package Legitimacy | 低风险；russh 有 Warp terminal 背书 |

---

## Open Questions (RESOLVED)

1. **disql CLI 参数格式**
   - What we know: disql 是达梦自带的 CLI 工具，用于执行 SQL
   - What's unclear: 批量执行 SQL 的具体参数格式（是 `-e "SQL;"` 还是通过 stdin pipe）
   - Recommendation: 在 Wave 0 测试任务中增加一个"验证 disql 批量 SQL 执行格式"的步骤，或在 CONTEXT 中要求 disql 通过 SSH stdin pipe 发送 SQL
   - **RESOLVED (2026-06-12):** 采用保守方案——通过 stdin pipe 发送 SQL：`echo "SP_SET_PARA_VALUE(1,'ALTER_MODE_STATUS',1);sp_set_oguid(453331);alter database primary;" | <install_path>/bin/disql SYSDBA/SYSDBA@<host>:<port>`。stdin pipe 兼容性最广，不依赖 disql `-e` 参数支持；多条 SQL 用分号分隔。Plan 03 deploy.rs configure_database_role 函数按此实现。

2. **全新安装 vs 备份还原初始化**
   - What we know: 多数生产文档使用 dmrman 备份还原确保主备数据一致；但全新安装（无历史数据）可以分别 dminit
   - What's unclear: Phase 3 MVP 是否允许假设"全新空库"，不支持从已有主库搭建备库
   - Recommendation: 在 CONTEXT.md 中已明确 Phase 3 是 MVP，可假设全新安装；在文档中注明此限制
   - **RESOLVED (2026-06-12):** Phase 3 MVP 假设"全新安装，无历史数据"——两节点分别 dminit 即可。不支持从已有主库搭建备库（需要 dmrman 备份还原，移至 Phase v2）。此假设作为已知限制记录于 03-03-SUMMARY.md 与 ROADMAP Phase 3 验收说明。

3. **dmserver mount 后台启动方式**
   - What we know: 需要 SSH exec `dmserver dm.ini mount` 后让进程在后台持续运行
   - What's unclear: SSH exec 命令结束后，远端进程是否继续运行？（通常 SSH session 关闭会发 SIGHUP）
   - Recommendation: 使用 `nohup dmserver dm.ini mount &` 或 `systemd-run --unit=dm-cluster dmserver dm.ini mount`（后者更可靠但需要 systemd）；也可通过 SSH `RequestPty` 建立伪终端保持会话
   - **RESOLVED (2026-06-12):** 采用 `nohup <install_path>/bin/dmserver <data_path>/<instance>/dm.ini mount > /tmp/dmserver_<instance>.log 2>&1 &` 模式。`nohup` 屏蔽 SIGHUP 保证 SSH session 关闭后进程存活；`&` 后台运行；stdout/stderr 重定向到日志文件便于排查。systemd-run 方案因依赖 systemd 不在 MVP 范围。Plan 03 deploy.rs start_dmserver_mount 函数按此实现。

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust/cargo | 编译 russh 依赖 | ✓ | 1.96.0 | — |
| Docker | 集成测试（模拟两节点 SSH） | ✓ | 29.5.3 | 手动测试 |
| SSH client | 开发调试 | ✓ | OpenSSH 10.2p1 | — |
| 达梦安装包（本地路径） | Phase 3 安装流程 | ✗ | — | 用户自备（DOWN-01 是 v2 需求） |
| 真实 Linux 目标节点 | 集成/端到端测试 | ✗ | — | Docker SSH 容器模拟 |

**Missing dependencies with no fallback:**
- 达梦安装包（DMInstall.bin/ISO）：测试时需要用户自备，或 mock SSH exec 返回

**Missing dependencies with fallback:**
- 真实 Linux 节点：可用 Docker `sshd` 容器模拟 SSH 预检查和 SFTP 上传；DMInstall.bin 执行可用 mock 替代

---

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust 内建 `#[test]` + cargo-nextest |
| Config file | Cargo.toml（无独立 test config） |
| Quick run command | `cargo test` |
| Full suite command | `cargo nextest run` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CLUS-01 (config) | `ClusterConfig` 从 TOML 正确反序列化 | unit | `cargo test config::cluster` | ❌ Wave 0 |
| CLUS-01 (tmpl) | dmmal.ini 模板主备内容一致 | unit | `cargo test cluster::templates` | ❌ Wave 0 |
| CLUS-01 (tmpl) | dmarch.ini 模板主备 ARCH_DEST 方向相反 | unit | `cargo test cluster::templates::dmarch` | ❌ Wave 0 |
| CLUS-01 (ssh) | SshError 类型覆盖连接/命令/SFTP 失败场景 | unit | `cargo test cluster::ssh` | ❌ Wave 0 |
| CLUS-02 | TCP 轮询 60s 超时后返回 Err | unit | `cargo test cluster::health::timeout` | ❌ Wave 0 |
| QUAL-01 | 预检查全通过时返回 Ok | unit (mock SSH) | `cargo test cluster::preflight::all_pass` | ❌ Wave 0 |
| QUAL-01 | 预检查单项失败时返回 Err 含节点信息 | unit (mock SSH) | `cargo test cluster::preflight::one_fail` | ❌ Wave 0 |
| CLUS-01/02 | 端到端：Docker SSH 容器完整流程 | integration (manual gate) | 需要真实环境，manual-only | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `cargo nextest run`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/config/cluster.rs` — `ClusterConfig`/`NodeConfig`/`SshCredentials` 结构体和反序列化测试
- [ ] `src/cluster/templates/mod.rs` — 所有模板生成函数的单元测试
- [ ] `src/cluster/preflight.rs` — 预检查函数接受可注入的"命令执行器" trait，便于 mock
- [ ] `src/cluster/health.rs` — `wait_tcp_ready` 超时路径单元测试
- [ ] `tests/fixtures/cluster_valid.toml` — 完整集群 TOML 示例（用于集成测试）
- [ ] `tests/fixtures/cluster_invalid_no_primary.toml` — 无 primary 节点时验证失败

---

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | yes | russh 密钥认证优先（D-06）；password 仅备用 |
| V3 Session Management | no | SSH 会话由 russh 管理，无 HTTP session |
| V4 Access Control | partial | 预检查 sudo 免密验证（QUAL-01）；不处理 DAC/MAC |
| V5 Input Validation | yes | TOML 反序列化 + `validate_cluster_config()` 检查端口范围、路径非空 |
| V6 Cryptography | yes | 使用 rustls backend（不用 OpenSSL）；禁止手写加密 |

### Known Threat Patterns for SSH Cluster Installer

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| 中间人攻击（MITM） | Spoofing | D-07 TOFU 接受初次连接主机密钥；生产环境建议人工预验证，Phase 3 MVP 接受此风险 |
| SSH 凭据泄露 | Information Disclosure | `identity_file` 路径在内存，不打印到日志；`password` 字段用 `#[serde(skip_serializing)]` |
| 远端命令注入 | Tampering | 配置文件字段通过 Rust struct（不拼接 shell 字符串）；SFTP 路径使用 `PathBuf::join` |
| 安装包完整性 | Tampering | Phase 2 已有 SHA-256 校验；集群模式复用 `checksum::verify_sha256` |

---

## Sources

### Primary (HIGH confidence)
- [crates.io russh](https://crates.io/crates/russh) — 版本 0.61.2 确认，总下载 4,037,306
- [crates.io russh-sftp](https://crates.io/crates/russh-sftp) — 版本 2.3.0 确认，总下载 1,667,580，发布 2026-05-23
- [docs.rs russh client Handler](https://docs.rs/russh/latest/russh/client/trait.Handler.html) — `check_server_key` 签名和语义
- [docs.rs russh-sftp SftpSession](https://docs.rs/russh-sftp/2.3.0/russh_sftp/client/struct.SftpSession.html) — `write()`/`create_dir()` 等方法确认
- [eco.dameng.com 配置文件说明](https://eco.dameng.com/document/dm/zh-cn/pm/configuration-description.html) — dm.ini/dmmal.ini/dmarch.ini/dmwatcher.ini 字段权威说明
- Phase 2 代码（`src/install/silent_install.rs`）— XML 生成模式，`build_dminit_command` 模式

### Secondary (MEDIUM confidence)
- [cnblogs.com/Williamls/p/17088364.html](https://www.cnblogs.com/Williamls/p/17088364.html) — dmmal.ini/dmarch.ini/dmwatcher.ini 完整示例（配置字段经 eco.dameng.com 交叉验证）
- [cnblogs.com/ios9/p/17696217.html](https://www.cnblogs.com/ios9/p/17696217.html) — 主备部署步骤序列（SP_SET_OGUID + ALTER DATABASE 命令）
- [eco.dameng.com 主备搭建](https://eco.dameng.com/community/article/a9f162dbea5d6ad703ce3bdb2b36094d) — mount 模式启动、OGUID 设置流程

### Tertiary (LOW confidence)
- russh GitHub examples（`client_exec_simple.rs`、`sftp_client.rs`）— 无法直接访问原始代码，基于 WebFetch 摘要推断 API 模式

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — russh/russh-sftp 版本经 crates.io 直接确认；所有其他依赖已在 Phase 2 Cargo.toml 中
- Architecture (russh API): MEDIUM — docs.rs 方法签名确认，但完整示例未能直接读取源码
- 达梦配置文件格式: MEDIUM — 官方文档 + 多篇社区文章交叉验证，核心字段（MAL_INST_NAME、ARCH_DEST、INST_OGUID）一致
- 部署步骤序列: MEDIUM — SP_SET_OGUID / ALTER DATABASE 命令经多篇资料验证
- Pitfalls: HIGH — 多数来自 Phase 2 已验证经验（dminit 参数格式）或官方文档约束

**Research date:** 2026-06-12
**Valid until:** 2026-07-12（russh-sftp 2.3.0 发布于 2026-05-23，相对新，如项目延迟应重新确认版本）
