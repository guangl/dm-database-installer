# Phase 7: DSC 共享存储集群 - Research

**Researched:** 2026-06-15
**Domain:** 达梦数据库 DSC (Data Sharing Cluster) 共享存储集群部署
**Confidence:** MEDIUM

---

## Summary

DSC（共享存储集群）是达梦数据库的多实例单库架构，与主备集群有根本性架构差异：多个数据库实例同时访问同一套共享物理文件（数据文件、控制文件、重做日志均在共享存储上），不依赖 dmwatcher/dmmonitor 进行日志同步。集群通过 DMCSS（集群同步服务）和 DMASM（自动存储管理）组件协调各节点。

DSC 部署流程分四个主要层次：
1. **存储层初始化**：使用 `dmasmcmd` 初始化 DCR 磁盘和表决磁盘，使用 `dmasmtool` 创建 DMLOG/DMDATA 磁盘组——仅在第一个节点执行
2. **服务层启动**：所有节点按 dmcss → dmasmsvr → dmserver 顺序启动
3. **数据库层初始化**：第一节点执行 `dminit`（PATH 指向 ASM 磁盘组路径 `+DMDATA/...`），生成各实例的 config 目录；其余节点复制对应 config 目录后直接启动
4. **验证层**：通过 disql 查询 `V$INSTANCE` 确认各节点状态为 OPEN 且 MODE$ 为 NORMAL（DSC 节点均为 NORMAL，非 PRIMARY/STANDBY）

与现有主备集群代码相比，DSC 完全不使用 `dmrman 备份/还原`、`dmwatcher`、`dmmonitor`、`alter database primary/standby` 这些步骤。但需要新增 `dmdcr_cfg.ini`、`dmasvrmal.ini`、`dmdcr.ini` 三类配置文件的生成，以及 DMCSS/DMASM 服务的注册逻辑。

**Primary recommendation:** 以 `src/cluster/dsc/mod.rs` 为入口，新增 `src/cluster/dsc/` 下的 templates 和 deploy 子模块；复用 `phases.rs` 中的 `run_preflight`、`run_install_phase`；所有 DSC 专有步骤（ASM 初始化、服务注册、dminit 共享路径）在 DSC 专有函数中实现。

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| dmdcr_cfg.ini 生成 | API / Backend（控制机） | — | 集群统一配置，由安装器在本地生成后 SFTP 推送到各节点 |
| dmasmcmd 磁盘初始化 | 第一节点（远程执行） | — | 仅在 first_node 上执行，通过 SSH CommandRunner |
| dmasmtool 磁盘组创建 | 第一节点（远程执行） | — | 需要 DMASM 服务先启动，在 first_node 交互式执行 |
| DMCSS/DMASM 服务注册 | 每个节点（并行） | — | 使用 dm_service_installer.sh，每节点独立注册 |
| dminit 共享存储初始化 | 第一节点（远程执行） | — | PATH 指向 +DMDATA ASM 路径，仅执行一次 |
| config 目录分发 | API / Backend（控制机） | — | first_node 生成的 dsc1_config 目录 SFTP 推送到其余节点 |
| dmserver 启动验证 | 每个节点（并行后顺序验证） | — | 等待 TCP 端口就绪，V$INSTANCE 查询 |
| Checkpoint 管理 | API / Backend（控制机） | — | 复用现有 ClusterCheckpoint 结构，添加 DSC 专有字段 |

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| DSC-01 | 用户可执行 `dm-installer install dsc` 完成 DSC 共享存储集群完整部署 | 实现 `dsc::run` 函数，调用 DSC 专有步骤链 |
| DSC-02 | 安装流程在所有节点自动调用 dmasmtool 初始化 ASM 磁盘组 | 新增 `run_asm_init_phase` 步骤，通过 SSH 在 first_node 执行 dmasmtool 命令序列 |
| DSC-03 | 第一节点执行 dminit（路径指向共享存储），其他节点直接挂载启动 | 新增 `run_dminit_shared_phase`（仅 first_node）和 `run_distribute_dsc_config_phase`（其余节点复制 config 目录） |
</phase_requirements>

---

## Standard Stack

### 核心（全部已在 Cargo.toml 中）

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `tokio` | 1.52.3 | 异步运行时 | 已有 |
| `russh` + `russh-sftp` | 0.61.2 / 2.3.0 | SSH/SFTP 执行远程命令 | 已有，复用 CommandRunner trait |
| `anyhow` / `thiserror` | 1.0.102 / 2.0.18 | 错误处理 | 已有 |
| `serde` + `toml` | 1.0.228 / 1.1.2 | TOML 配置反序列化 | 已有 |
| `futures` | 0.3 | `try_join_all` 并行异步 | 已有 |
| `tracing` | 0.1.44 | 结构化日志 | 已有 |

**无需新增任何外部 crate。** [VERIFIED: 代码库检查]

### 不适用的现有组件（DSC 不使用）

| 现有组件 | 原因 |
|---------|------|
| `dmrman` 备份/还原 | DSC 节点共享同一份数据，不需要数据同步 |
| `dmwatcher` / `dmmonitor` | DSC 通过 DMCSS 协调，无需守护进程 |
| `alter database primary/standby` | DSC 节点角色均为 NORMAL，不区分主备 |
| `dmmal.ini` / `dmwatcher.ini` / `dmarch.ini` | DSC 使用 `dmdcr_cfg.ini` + `dmasvrmal.ini` + `dmdcr.ini` 替代 |

---

## Package Legitimacy Audit

本 Phase 不新增任何外部 package，全部依赖已在 Cargo.toml 中存在并经过先前 Phase 验证。

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

---

## Architecture Patterns

### System Architecture Diagram

```
用户: dm-installer install dsc
         |
         v
    dsc::run(common, specific)
         |
    ┌────┴────────────────────────────────────────────────┐
    │  Phase 1: 建立 SSH 会话（所有节点）                    │
    └────────────────────────┬────────────────────────────┘
                             |
    ┌────────────────────────┴────────────────────────────┐
    │  Phase 2: 预检查（sudo 免密 / 端口 / 磁盘）           │
    │           复用 phases::run_preflight()              │
    └────────────────────────┬────────────────────────────┘
                             |
    ┌────────────────────────┴────────────────────────────┐
    │  Phase 3: 安装 DM 软件包（所有节点并行）              │
    │           复用 phases::run_install_phase()          │
    │           注意：DSC 无需 dminit（此步骤跳过 dminit） │
    └────────────────────────┬────────────────────────────┘
                             |
    ┌────────────────────────┴────────────────────────────┐
    │  Phase 4: 生成并分发 DSC 配置文件（所有节点）          │
    │   dmdcr_cfg.ini → 推送所有节点                        │
    │   dmasvrmal.ini → 推送所有节点                        │
    │   dmdcr.ini(seqno=N) → 推送各自节点                  │
    └────────────────────────┬────────────────────────────┘
                             |
    ┌────────────────────────┴────────────────────────────┐
    │  Phase 5: 启动 DMCSS 和 DMASM 服务（所有节点并行）   │
    │   dm_service_installer.sh -t dmcss                 │
    │   dm_service_installer.sh -t dmasmsvr              │
    └────────────────────────┬────────────────────────────┘
                             |
    ┌────────────────────────┴────────────────────────────┐
    │  Phase 6: ASM 初始化（仅 first_node）                │
    │   dmasmcmd: create dcrdisk, create votedisk         │
    │             create asmdisk, init dcrdisk, init vtd  │
    │   dmasmtool: create diskgroup DMLOG, DMDATA         │
    └────────────────────────┬────────────────────────────┘
                             |
    ┌────────────────────────┴────────────────────────────┐
    │  Phase 7: dminit 共享存储初始化（仅 first_node）      │
    │   dminit control=<dminit.ini>                       │
    │   PATH 指向 +DMDATA/...（ASM 磁盘组）                │
    └────────────────────────┬────────────────────────────┘
                             |
    ┌────────────────────────┴────────────────────────────┐
    │  Phase 8: 分发 config 目录（first_node → other 节点）│
    │   SFTP 下载 dsc1_config（及更多节点的 dscN_config）  │
    │   SFTP 上传到对应节点                                │
    └────────────────────────┬────────────────────────────┘
                             |
    ┌────────────────────────┴────────────────────────────┐
    │  Phase 9: 注册并启动 dmserver 服务（所有节点有序）     │
    │   dm_service_installer.sh -t dmserver               │
    │   先启动第一节点，等待 TCP 就绪                        │
    │   再启动其余节点                                      │
    └────────────────────────┬────────────────────────────┘
                             |
    ┌────────────────────────┴────────────────────────────┐
    │  Phase 10: 验证集群状态                               │
    │   V$INSTANCE: STATUS$=OPEN, MODE$=NORMAL            │
    └────────────────────────────────────────────────────┘
```

### Recommended Project Structure

```
src/cluster/dsc/
├── mod.rs              # 入口：run() + run_with_runners()，Checkpoint 控制流
├── templates.rs        # 生成 dmdcr_cfg.ini / dmasvrmal.ini / dmdcr.ini / dminit.ini
└── deploy.rs           # DSC 专有部署函数：asm_init / dminit_shared / distribute_config / start_services
```

现有 `src/cluster/phases.rs` 中的 `run_preflight` 和 `run_install_phase` 可直接复用（但需要 `run_install_phase` 跳过 dminit，见下文 Pitfall 部分）。

### DSC 配置文件结构 [ASSUMED: 基于多篇社区文档综合]

**dmdcr_cfg.ini**（集群拓扑，所有节点相同）：
```ini
DCR_N_GRP = 3
DCR_VTD_PATH = /dev/raw/raw2        # 表决磁盘（或用户指定的块设备）
DCR_OGUID = 63635                   # 对应 config 中的 oguid

[GRP]
DCR_GRP_TYPE = CSS
DCR_GRP_N_EP = 2
DCR_GRP_DSKCHK_CNT = 60
  [EP0]
  DCR_EP_HOST = 192.168.1.10
  DCR_EP_PORT = 9341
  [EP1]
  DCR_EP_HOST = 192.168.1.11
  DCR_EP_PORT = 9343

[GRP]
DCR_GRP_TYPE = ASM
DCR_GRP_N_EP = 2
DCR_GRP_DSKCHK_CNT = 60
  [EP0]
  DCR_EP_ASM_LOAD_PATH = /dev/raw
  DCR_EP_HOST = 192.168.1.10
  DCR_EP_PORT = 9349
  DCR_EP_ASM_SHMKEY = 93360
  [EP1]
  DCR_EP_ASM_LOAD_PATH = /dev/raw
  DCR_EP_HOST = 192.168.1.11
  DCR_EP_PORT = 9351
  DCR_EP_ASM_SHMKEY = 93361

[GRP]
DCR_GRP_TYPE = DB
DCR_GRP_N_EP = 2
DCR_GRP_DSKCHK_CNT = 60
  [EP0]
  DCR_EP_HOST = 192.168.1.10
  DCR_EP_PORT = 5236
  DCR_EP_CHECK_PORT = 9741
  [EP1]
  DCR_EP_HOST = 192.168.1.11
  DCR_EP_PORT = 5237
  DCR_EP_CHECK_PORT = 9742
```

**dmasvrmal.ini**（所有节点相同）：
```ini
[MAL_INST0]
MAL_INST_NAME = DSC0
MAL_HOST = 192.168.1.10
MAL_PORT = 9349

[MAL_INST1]
MAL_INST_NAME = DSC1
MAL_HOST = 192.168.1.11
MAL_PORT = 9351
```

**dmdcr.ini**（各节点不同，DMDCR_SEQNO 区分）：
```ini
DMDCR_PATH = /dev/raw/raw1         # DCR 磁盘
DMDCR_MAL_PATH = <dmasvrmal.ini 路径>
DMDCR_SEQNO = 0                   # 节点 0 为 0，节点 1 为 1
DMDCR_ASM_RESTART_INTERVAL = 60
DMDCR_DB_RESTART_INTERVAL = 60
DMDCR_ASM_STARTUP_CMD = <dmasmsvr 路径> DCR_INI=<dmdcr.ini 路径>
DMDCR_DB_STARTUP_CMD = <dmserver 路径> <dm.ini 路径> dcr_ini=<dmdcr.ini 路径>
```

**dminit.ini**（共享存储路径，仅 first_node 使用）：
```ini
SYSDBA_PWD = <password>
DCR_PATH = /dev/raw/raw1
DCR_OGUID = 63635
DB_NAME = GRP_DSC
SYSTEM_PATH = +DMDATA/data         # + 前缀代表 ASM 磁盘组

[DSC0]
CONFIG_PATH = <本地config目录>/dsc0_config
PORT_NUM = 5236
MAL_HOST = 192.168.1.10
MAL_PORT = 5237
LOG_PATH = +DMLOG/log/dsc0_log01.log

[DSC1]
CONFIG_PATH = <本地config目录>/dsc1_config
PORT_NUM = 5237
MAL_HOST = 192.168.1.11
MAL_PORT = 5238
LOG_PATH = +DMLOG/log/dsc1_log01.log
```

### Pattern 1: DSC 节点角色建模

DSC 集群无主备角色区分，但需要区分"第一节点"（负责磁盘初始化和 dminit）与"其余节点"：

```rust
// [ASSUMED] 从 runners 中取第一个作为 first_node
fn first_node(runners: &phases::Runners) -> Option<&(NodeConfig, Arc<dyn CommandRunner>)> {
    runners.first()
}

fn other_nodes(runners: &phases::Runners) -> &[(NodeConfig, Arc<dyn CommandRunner>)] {
    runners.split_first().map(|(_, rest)| rest).unwrap_or(&[])
}
```

现有 `NodeRole` enum 需要考虑是否扩展：DSC 场景下 `Primary` 可映射为"第一节点"（负责初始化），`Standby` 可映射为"其余节点"。这样可以复用现有 TOML 解析，无需修改 `NodeRole`。[ASSUMED]

### Pattern 2: dmasmtool 交互式命令处理

`dmasmtool` 需要进入交互模式执行多条命令，通过 stdin 传入：

```bash
# 实际命令格式（通过 SSH heredoc 方式执行）
{install_path}/bin/dmasmtool DCR_INI={dmdcr_ini_path} << 'EOF'
create diskgroup 'DMLOG' asmdisk '{log_disk_path}'
create diskgroup 'DMDATA' asmdisk '{data_disk_path}'
EOF
```

使用 SSH CommandRunner 的 `exec` 直接传递带 heredoc 的 shell 命令即可，无需额外处理。[ASSUMED: 基于社区文档，dmasmtool 支持 stdin 管道输入]

### Pattern 3: dm_service_installer.sh 注册 DSC 服务

```bash
# 注册 DMCSS 服务
bash {install_path}/script/root/dm_service_installer.sh \
  -t dmcss -dcr_ini {dmdcr_ini_path} -p DMCSS

# 注册 DMASM 服务（依赖 DMCSS）
bash {install_path}/script/root/dm_service_installer.sh \
  -t dmasmsvr -dcr_ini {dmdcr_ini_path} -p DMASM \
  -y DmCSSServiceDMCSS

# 注册 dmserver 服务（依赖 DMASM）
bash {install_path}/script/root/dm_service_installer.sh \
  -t dmserver -dm_ini {dm_ini_path} -dcr_ini {dmdcr_ini_path} \
  -p DMSERVER -y DmASMSvrServiceDMASM
```

[CITED: 达梦社区 - dm_service_installer.sh 参数说明；博客园 xuchuangye]

### Pattern 4: DSC 专有 Checkpoint 结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DscCheckpoint {
    pub preflight_done: bool,
    pub install_done: bool,
    pub dsc_config_distributed: bool,  // DSC 配置文件已推送到各节点
    pub css_asm_started: bool,          // DMCSS+DMASM 已在所有节点启动
    pub asm_diskgroup_created: bool,    // first_node ASM 磁盘组已创建
    pub dminit_done: bool,              // first_node dminit 已执行
    pub config_dir_distributed: bool,   // dscN_config 已分发到各节点
    pub dmserver_started: bool,         // 所有节点 dmserver 已启动
}
```

[ASSUMED: 结构基于 DSC 部署步骤设计，与现有 ClusterCheckpoint 模式一致]

### Anti-Patterns to Avoid

- **anti-pattern: 在 DSC 中运行 dmrman 备份/还原** — DSC 所有节点共享同一数据，不需要数据同步
- **anti-pattern: 在 DSC 中配置 alter database primary/standby** — DSC 节点角色均为 NORMAL
- **anti-pattern: 在 DSC 中启动 dmwatcher/dmmonitor** — DSC 通过 DMCSS 管理，不使用 DW 守护进程
- **anti-pattern: DSC 中每个节点各自执行 dminit** — 仅 first_node 执行一次，其余节点接收 config 目录
- **anti-pattern: 复用 run_install_phase 中的 run_dminit_remote** — 需要跳过 dminit 或提供 DSC 专有版本

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SSH 远程命令执行 | 自定义 SSH 客户端 | 已有 CommandRunner trait + SshSession | 复用，全套测试覆盖 |
| SFTP 文件推送 | 自定义文件传输 | 已有 sftp_write/sftp_read | 复用 |
| TCP 健康检查等待 | 自定义轮询 | 已有 health::wait_tcp_ready | 复用 |
| Checkpoint 持久化 | 自定义状态文件 | 仿照 ClusterCheckpoint 模式 | 模式已验证 |
| Shell 命令注入防护 | 自定义转义 | 已有 common::shell_quote | 复用 |
| 并行任务执行 | 手写 Arc + JoinHandle | futures::future::try_join_all | 已有模式 |

---

## Common Pitfalls

### Pitfall 1: run_install_phase 内嵌 dminit 调用

**What goes wrong:** 现有 `run_install_phase` 在安装软件包后会自动对 `NodeRole::Primary` 节点执行 `run_dminit_remote`，而 DSC 中 dminit 需要特殊参数（通过 dminit.ini 文件传入、PATH 指向 ASM 路径），不能使用普通 dminit 命令。

**Why it happens:** `phases::run_install_phase` 为主备集群设计，合并了安装和 dminit 两步。

**How to avoid:** DSC 版本的 install phase 应仅执行软件包安装，跳过 dminit；或新建 `run_dsc_install_phase` 只调用 `deploy::upload_installer_and_install` 而不调用 `deploy::run_dminit_remote`。

**Warning signs:** 如果 dminit 执行时 PATH 参数指向本地目录而非 ASM 路径，说明用错了函数。

### Pitfall 2: dmasmtool 需要 DMASM 服务已启动

**What goes wrong:** 在 DMASM 服务 (`dmasmsvr`) 启动之前调用 `dmasmtool` 会报连接 ASM 失败的错误。

**Why it happens:** dmasmtool 通过共享内存与 dmasmsvr 通信，服务未就绪则命令失败。

**How to avoid:** 严格保持顺序：先 `systemctl start DmCSSServiceDMCSS` → 等待就绪 → `systemctl start DmASMSvrServiceDMASM` → 等待就绪 → 执行 dmasmtool。

**Warning signs:** dmasmtool 输出 "connect to asm server failed" 类似错误。

### Pitfall 3: 各节点 dmdcr.ini 中 DMDCR_SEQNO 必须不同

**What goes wrong:** 所有节点使用相同的 dmdcr.ini（DMDCR_SEQNO 相同）导致集群注册冲突，DMCSS 报节点 ID 重复。

**Why it happens:** 每个节点在集群中有唯一的序号（从 0 开始），通过 SEQNO 区分。

**How to avoid:** 生成 dmdcr.ini 时按节点在 `runners` 中的下标设置 `DMDCR_SEQNO = {index}`，并分别 SFTP 推送到各自节点。

**Warning signs:** DMCSS 启动日志中出现 "seqno conflict" 或节点无法加入集群。

### Pitfall 4: 共享存储路径的 + 前缀语法

**What goes wrong:** dminit.ini 中 `SYSTEM_PATH = /dev/sdc` 会被认为是本地路径，而非 ASM 磁盘组。应写成 `SYSTEM_PATH = +DMDATA/data`（`+` 开头代表 ASM 磁盘组名）。

**Why it happens:** DM ASM 使用 Oracle ASM 风格的路径语法，`+DISKGROUP_NAME/...` 表示 ASM 路径。

**How to avoid:** 从 `ClusterSpecificConfig.shared_storage` 读取磁盘组名（如 `DMDATA`），在 dminit.ini 生成时自动加 `+` 前缀。用户在 config 中填写磁盘组名（不含 `+`），工具负责格式化。

**Warning signs:** dminit 报 "路径不存在" 或 "不支持的存储类型"。

### Pitfall 5: V$INSTANCE 验证预期结果与主备不同

**What goes wrong:** 验证 DSC 节点时沿用主备集群的期望值（PRIMARY/STANDBY），导致验证失败即使集群已正常。

**Why it happens:** DSC 所有节点的 MODE$ 均为 NORMAL，而非 PRIMARY/STANDBY。

**How to avoid:** DSC 的 `verify_phase` 应检查 `STATUS$=OPEN` 且 `MODE$=NORMAL`（或未出现 STANDBY 字样即可接受）。

**Warning signs:** disql 输出包含 OPEN 和 NORMAL 但当前验证逻辑因找不到 PRIMARY 而报错。

### Pitfall 6: dmasmcmd 初始化操作幂等性

**What goes wrong:** 重跑部署时 `dmasmcmd create dcrdisk` 在磁盘已初始化的情况下可能报错，导致 checkpoint 不可恢复。

**Why it happens:** 部分 dmasmcmd 操作在目标已存在时会失败（非幂等）。

**How to avoid:** Checkpoint 中记录 `asm_diskgroup_created: bool`，跳过已完成的 ASM 初始化步骤。如果可能，在 dmasmcmd 命令前检查磁盘状态（`list diskgroup` 等）。

---

## Code Examples

### Example 1: dmasmcmd 初始化序列（first_node 执行）

```rust
// [ASSUMED] 基于社区文档的命令格式
pub async fn run_dmasmcmd_init(
    install_path: &str,
    dcr_disk: &str,     // 例：/dev/raw/raw1
    vote_disk: &str,    // 例：/dev/raw/raw2
    log_disk: &str,     // 例：/dev/raw/raw3
    data_disk: &str,    // 例：/dev/raw/raw4
    cfg_path: &str,     // dmdcr_cfg.ini 在节点上的路径
    runner: &dyn CommandRunner,
) -> Result<()> {
    let cmds = format!(
        "create dcrdisk '{dcr_disk}' 'dcr'\n\
         create votedisk '{vote_disk}' 'vote'\n\
         create asmdisk '{log_disk}' 'LOG0'\n\
         create asmdisk '{data_disk}' 'DATA0'\n\
         init dcrdisk '{dcr_disk}' from '{cfg_path}'\n\
         init votedisk '{vote_disk}' from '{cfg_path}'\n"
    );
    let cmd = format!(
        "echo '{}' | {}/bin/dmasmcmd",
        cmds,
        shell_quote(install_path)
    );
    let (stdout, exit_code) = runner.exec(&cmd).await?;
    anyhow::ensure!(exit_code == 0, "dmasmcmd 初始化失败: {}", String::from_utf8_lossy(&stdout));
    Ok(())
}
```

### Example 2: dmasmtool 创建磁盘组（first_node，DMASM 启动后执行）

```rust
// [ASSUMED] 基于社区文档的命令格式
pub async fn run_dmasmtool_create_diskgroups(
    install_path: &str,
    dmdcr_ini_path: &str,
    log_disk: &str,
    data_disk: &str,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let cmds = format!(
        "create diskgroup 'DMLOG' asmdisk '{log_disk}'\n\
         create diskgroup 'DMDATA' asmdisk '{data_disk}'\n"
    );
    let cmd = format!(
        "echo '{}' | {}/bin/dmasmtool DCR_INI={}",
        cmds,
        shell_quote(install_path),
        shell_quote(dmdcr_ini_path)
    );
    let (stdout, exit_code) = runner.exec(&cmd).await?;
    anyhow::ensure!(exit_code == 0, "dmasmtool 创建磁盘组失败: {}", String::from_utf8_lossy(&stdout));
    Ok(())
}
```

### Example 3: 注册 DMCSS 服务（每节点）

```rust
// [CITED: 达梦社区 dm_service_installer.sh 参数说明]
pub async fn register_dmcss_service(
    install_path: &str,
    dmdcr_ini_path: &str,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let script = format!("{}/script/root/dm_service_installer.sh", install_path);
    let cmd = format!(
        "bash {} -t dmcss -dcr_ini {} -p DMCSS",
        shell_quote(&script),
        shell_quote(dmdcr_ini_path)
    );
    let (stdout, exit_code) = runner.exec(&cmd).await?;
    anyhow::ensure!(exit_code == 0, "dmcss 服务注册失败: {}", String::from_utf8_lossy(&stdout));
    start_and_enable_remote_service("DmCSSServiceDMCSS", runner).await
}
```

### Example 4: DSC V$INSTANCE 验证

```rust
// [ASSUMED] DSC 节点期望 MODE$=NORMAL, STATUS$=OPEN
pub async fn verify_dsc_node(
    node: &NodeConfig,
    dminit: &DminitConfig,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let cmd = format!(
        "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
        shell_quote(&dminit.install_path),
        shell_quote(&dminit.sysdba_password),
        dminit.port,
    );
    let (stdout, exit_code) = runner.exec(&cmd).await?;
    anyhow::ensure!(exit_code == 0, "V$INSTANCE 查询失败: {}", String::from_utf8_lossy(&stdout));
    let output = String::from_utf8_lossy(&stdout);
    anyhow::ensure!(
        output.contains("OPEN"),
        "DSC 节点 {} 未达到 OPEN 状态，实际输出:\n{}",
        node.host, output
    );
    tracing::info!("[node:{}] DSC 节点验证通过 STATUS$=OPEN", node.host);
    Ok(())
}
```

---

## State of the Art

| 旧做法 | 当前做法 | 影响 |
|--------|---------|------|
| DSC stub 直接 bail! | 完整实现 dsc::run() | Phase 7 目标 |
| DSC_SPECIFIC 模板含不准确字段 | 更新 init.rs 中 DSC_SPECIFIC 为准确的 DCR 配置格式 | 用户体验改善 |
| ClusterCheckpoint 只有主备字段 | 新增 DscCheckpoint 或在现有 ClusterCheckpoint 中扩展 DSC 字段 | 支持 DSC 断点续传 |

**Deprecated/outdated:**
- `src/config/init.rs` 中的 `DSC_SPECIFIC` 模板：当前内容不反映真实的 DSC 配置格式（如缺少 dcr_disk / css_port 等 DSC 专有字段），Phase 7 应更新

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | dmasmtool 支持通过 stdin pipe 传入命令（echo \| dmasmtool）| Code Examples §2 | 若不支持，需通过临时文件传入命令脚本 |
| A2 | dmasmcmd 通过 echo \| dmasmcmd 传入命令序列 | Code Examples §1 | 若不支持交互式 pipe，需换用 expect 或脚本文件方式 |
| A3 | DSC 节点 V$INSTANCE.MODE$ = NORMAL（非 PRIMARY/STANDBY）| Common Pitfalls §5 | 若实际为其他值（如 DSC0/DSC1），验证逻辑需调整 |
| A4 | NodeRole::Primary 映射为 DSC first_node（负责磁盘初始化和 dminit）| Architecture Patterns | 若团队决定使用新字段，需修改 NodeRole enum 和 config 解析 |
| A5 | dminit.ini 中 SYSTEM_PATH 使用 +DISKGROUPNAME 语法 | Architecture Patterns | 若用户提供的是 raw 设备路径而非磁盘组名，需要调整路径格式化逻辑 |
| A6 | DMCSS 服务注册命令参数 `-dcr_ini`（非 `-i`）对 dmcss 类型有效 | Code Examples §3 | 若参数名不同，注册命令会失败；运行时可见错误信息 |
| A7 | dminit 在 DSC 中通过 `control=<ini文件>` 参数接受 ini 文件（非命令行参数列表方式）| Architecture Patterns | 若 DSC dminit 也支持命令行参数方式，可以沿用 build_dminit_args 模式 |

---

## Open Questions (RESOLVED)

1. **dmasmtool/dmasmcmd 是否支持 stdin pipe 方式**
   - What we know: 社区文档均显示在交互式 shell 中逐行输入命令
   - What's unclear: 是否有非交互式批量执行方式（echo pipe / 脚本文件）
   - Recommendation: 实现时优先使用临时脚本文件方式（写到 /tmp/dsc_asm_init.sh，chmod +x 执行），更稳健
   - **RESOLVED:** 使用 `printf '%s\n' cmd1 cmd2 ... | dmasmcmd` 方式传入命令序列，比 echo 更安全可靠；deploy.rs 已按此实现。

2. **DSC 中 config 目录分发方式**
   - What we know: first_node dminit 后生成 dsc0_config、dsc1_config 等多个目录
   - What's unclear: 目录结构是否可以通过 SFTP 递归传输，或需要先 tar 打包
   - Recommendation: 先在 first_node `tar czf /tmp/dsc1_config.tar.gz dsc1_config`，SFTP 下载，上传到 node1，解压
   - **RESOLVED:** 采用 Recommendation 方案：first_node tar 打包 → sftp_read 下载 → sftp_write 上传到 other_node → 远端 tar 解压。deploy.rs::distribute_config_dir 已按此实现。

3. **DSC 中 NodeRole 使用方式**
   - What we know: DSC 无主备之分，但代码中 NodeRole::Primary 表示第一节点
   - What's unclear: 用户 config 中应该如何表达 DSC 节点（primary/standby 对应 first/other，还是引入新角色）
   - Recommendation: 沿用 primary=first_node、standby=other_nodes 映射，避免修改 NodeRole enum 和 config 解析代码，并在 DSC 模板注释中说明语义
   - **RESOLVED:** 沿用 NodeRole::Primary = first_node、NodeRole::Standby = other_nodes 映射；不新增 enum 变体，在 dsc/mod.rs 注释中说明 DSC 语义与主备语义的差异。

4. **shared_storage 字段含义的精确化**
   - What we know: `ClusterSpecificConfig.shared_storage` 现在只有一个字符串
   - What's unclear: 用户需要配置多个磁盘（dcr_disk、vote_disk、log_disk、data_disk），一个字段不够
   - Recommendation: 在 dsc.toml 中扩展配置结构，支持 `[dsc_storage]` 段包含 dcr_disk、vote_disk、log_disk、data_disk 四个字段；或在 `ClusterSpecificConfig` 中新增 `DscStorageConfig` 可选字段
   - **RESOLVED:** 在 ClusterSpecificConfig 中新增 `dsc_storage: Option<DscStorageConfig>` 字段（Plan 01 已实现），validate_dsc 校验该字段存在，用户在 dsc.toml 中配置 `[dsc_storage]` 段。

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | 构建 | ✓ | 项目已有 | — |
| russh + russh-sftp | SSH 执行远程命令 | ✓ | 0.61.2 / 2.3.0（已在 Cargo.toml）| — |
| 目标节点：dmasmcmd | ASM 磁盘初始化 | 运行时 | 随 DM 安装包提供 | — |
| 目标节点：dmasmtool | 磁盘组创建 | 运行时 | 随 DM 安装包提供 | — |
| 目标节点：dmcss | 集群同步服务 | 运行时 | 随 DM 安装包提供 | — |
| 目标节点：共享块设备 | DCR/VOTE/LOG/DATA 磁盘 | 用户环境 | — | 无（用户必须提前准备） |

**Missing dependencies with no fallback:**
- 共享块设备（raw device）：必须由用户在 DM 安装前准备好，安装器无法自动创建。应在 preflight 中检查 `ls {dcr_disk}` 等路径是否存在。

---

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (内置) + tracing-test |
| Config file | Cargo.toml [dev-dependencies] |
| Quick run command | `cargo test -p dm-database-installer -- cluster::dsc` |
| Full suite command | `cargo test -p dm-database-installer` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DSC-01 | `dsc::run` 完整流程不 panic，Checkpoint 控制各步骤 | unit | `cargo test -p dm-database-installer -- dsc::tests` | ❌ Wave 0 |
| DSC-02 | `run_asm_init_phase` 调用 dmasmcmd+dmasmtool 命令，mock 验证 | unit | `cargo test -p dm-database-installer -- dsc::deploy::tests` | ❌ Wave 0 |
| DSC-03 | `run_dminit_shared_phase` 仅在 first_node 执行，其他节点收到 config 目录 | unit | `cargo test -p dm-database-installer -- dsc::deploy::tests` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p dm-database-installer 2>&1 | tail -5`
- **Per wave merge:** `cargo test -p dm-database-installer`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/cluster/dsc/deploy.rs` — DSC 专有部署函数（含 mock 测试）
- [ ] `src/cluster/dsc/templates.rs` — dmdcr_cfg.ini / dmasvrmal.ini / dmdcr.ini 生成函数
- [ ] `src/cluster/dsc/mod.rs` 完整实现（替换 bail!）

---

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | yes | SSH 凭据复用现有 SshCredentials 结构 |
| V3 Session Management | yes | russh 会话管理（已有） |
| V4 Access Control | yes | sudo 免密检查（已有 preflight） |
| V5 Input Validation | yes | shell_quote 防注入（已有 common::shell_quote） |
| V6 Cryptography | no | 无新增加密操作 |

### Known Threat Patterns for DSC Stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| 磁盘路径注入（dcr_disk 字段包含特殊字符） | Tampering | shell_quote 包裹所有路径参数 |
| SSH 会话劫持 | Spoofing | russh 使用主机密钥验证（继承已有安全设置） |
| 共享存储越权访问 | Elevation | preflight 检查 sudo 权限，由用户保证节点网络隔离 |

---

## Sources

### Primary (HIGH confidence)
- [达梦技术社区 - 共享存储集群官方文档](https://eco.dameng.com/document/dm/zh-cn/start/dm-asm-cluster.html) - DSC 架构概述
- 项目代码库 `src/cluster/` - 现有集群部署模式（VERIFIED: 直接读取代码）
- 项目代码库 `src/config/cluster.rs` - ClusterSpecificConfig + validate_dsc（VERIFIED: 直接读取代码）

### Secondary (MEDIUM confidence)
- [达梦社区 - 在本地虚拟机上部署DMDSC](https://eco.dameng.com/community/article/f602b0a7667c03a20328d3aadd3bd29e) - 完整部署步骤，含 dmdcr_cfg.ini 示例
- [CSDN - DMDSC集群部署](https://blog.csdn.net/qq_45222081/article/details/126980816) - dmasmcmd 命令序列、dminit.ini 格式
- [博客园 - 达梦共享集群dsc+实时主备搭建](https://www.cnblogs.com/xuchuangye/p/14755333.html) - dm_service_installer.sh 参数（-t dmcss/-t dmasmsvr）

### Tertiary (LOW confidence)
- [达梦社区 - DM8 DSC集群安装部署指南（非镜像磁盘配置）](https://eco.dameng.com/community/training/9b6fe54647075f748883f2c51f5d20ec) - 磁盘组创建命令
- [达梦社区 - 试验DMDSC集群搭建](https://eco.dameng.com/community/training/dd08d4451dc2f229222190972e449954) - 两节点 dmdcr.ini 区别

---

## Metadata

**Confidence breakdown:**
- Standard Stack: HIGH - 无新依赖，全复用现有 Cargo.toml
- Architecture: MEDIUM - 基于多篇社区文档交叉验证，但命令格式细节有部分 ASSUMED 标记
- Pitfalls: MEDIUM - 基于文档分析和与主备集群的架构对比推导
- DSC 配置文件格式: MEDIUM - 多篇社区文章内容一致，但具体参数名未经官方文档 API 级验证

**Research date:** 2026-06-15
**Valid until:** 2026-07-15（达梦 DSC 部署流程相对稳定，30 天内有效）
