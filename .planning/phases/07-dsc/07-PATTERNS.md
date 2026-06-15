# Phase 7: DSC 共享存储集群 - Pattern Map

**Mapped:** 2026-06-15
**Files analyzed:** 5 (new/modified files)
**Analogs found:** 5 / 5

---

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/cluster/dsc/mod.rs` | cluster-entry | request-response (SSH orchestration) | `src/cluster/rws/mod.rs` | exact |
| `src/cluster/dsc/deploy.rs` | service | CRUD + event-driven (SSH remote exec) | `src/cluster/deploy.rs` | exact |
| `src/cluster/dsc/templates.rs` | utility | transform | `src/cluster/templates/dmmal_ini.rs` | exact |
| `src/cluster/checkpoint.rs` (extend) | model | CRUD | `src/cluster/checkpoint.rs` | exact (modify) |
| `src/config/cluster.rs` (extend) | config | CRUD | `src/config/cluster.rs` | exact (modify) |

---

## Pattern Assignments

### `src/cluster/dsc/mod.rs` (cluster-entry, request-response)

**Analog:** `src/cluster/rws/mod.rs`

**Imports pattern** (lines 1-6):
```rust
use anyhow::Result;

use crate::cluster::{health, phases};
use crate::common::ssh;
use crate::config::cluster::{ClusterSpecificConfig, DminitConfig};
use crate::config::CommonConfig;
```

**Entry point pattern** — `run()` 建立 SSH 连接再委托给 `run_with_runners()`（lines 8-28）:
```rust
pub async fn run(common: CommonConfig, specific: ClusterSpecificConfig) -> Result<()> {
    use std::sync::Arc;

    tracing::info!("[cluster][1/11] 建立 SSH 会话");
    let mut runners: phases::Runners = Vec::new();
    for node in &specific.nodes {
        let session = ssh::SshSession::connect(&node.host, node.ssh.port, &node.ssh)
            .await
            .map_err(|e| anyhow::anyhow!("连接节点 {}:{} 失败: {}", node.host, node.ssh.port, e))?;
        runners.push((node.clone(), Arc::new(session)));
    }
    run_with_runners(
        common,
        specific,
        runners,
        |host, port, secs| {
            Box::pin(async move { health::wait_tcp_ready(&host, port, secs).await })
        },
    )
    .await
}
```

**Testable core pattern** — `run_with_runners()` 接受注入的 runners 和 health_check_fn（lines 30-55）:
```rust
pub async fn run_with_runners<F>(
    common: CommonConfig,
    specific: ClusterSpecificConfig,
    runners: phases::Runners,
    health_check_fn: F,
) -> Result<()>
where
    F: Fn(String, u16, u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
        + Send
        + Sync,
{
    let dminit = specific.dminit.clone();
    let mut cp = crate::cluster::checkpoint::ClusterCheckpoint::load()?.unwrap_or_default();
    // ... checkpoint gate 控制流 ...
    crate::cluster::checkpoint::ClusterCheckpoint::remove()?;
    tracing::info!("集群部署完成");
    Ok(())
}
```

**Checkpoint gate 模式**（lines 57-113 in rws/mod.rs）— 每个 phase 前判断 cp.xxx_done，完成后 cp.save()：
```rust
async fn run_early_checkpoints(
    cp: &mut crate::cluster::checkpoint::ClusterCheckpoint,
    common: &CommonConfig,
    runners: &phases::Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    if !cp.preflight_done {
        phases::run_preflight(runners, dminit).await?;
        cp.preflight_done = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过预检查（checkpoint）");
    }
    if !cp.install_done {
        // DSC 专用：此处调用 dsc::deploy::run_install_only（跳过 dminit）
        phases::run_install_phase(common, runners, dminit).await?;
        cp.install_done = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过安装（checkpoint）");
    }
    Ok(())
}
```

**First node 选取模式**（RESEARCH.md Pattern 1）:
```rust
fn first_node(runners: &phases::Runners) -> Option<&(NodeConfig, Arc<dyn CommandRunner>)> {
    runners.first()
}

fn other_nodes(runners: &phases::Runners) -> &[(NodeConfig, Arc<dyn CommandRunner>)] {
    runners.split_first().map(|(_, rest)| rest).unwrap_or(&[])
}
```

注意：DSC 中 `NodeRole::Primary` = first_node（负责磁盘初始化和 dminit），`NodeRole::Standby` = 其他节点，语义通过注释说明，无需修改 `NodeRole` enum。

**并行执行模式**（复用 `rws/mod.rs` + `phases.rs` 中的 `try_join_all` 模式）:
```rust
let futs: Vec<_> = runners.iter().map(|(node, runner)| {
    let node = node.clone();
    let runner = Arc::clone(runner);
    async move { some_dsc_fn(&node, runner.as_ref()).await }
}).collect();
futures::future::try_join_all(futs).await?;
```

**测试模式**（lines 115-139 in rws/mod.rs）:
```rust
#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    #[test]
    fn test_checkpoint_gate_skips_done_phases() {
        let dir = TempDir::new().unwrap();
        let cp = crate::cluster::checkpoint::ClusterCheckpoint {
            preflight_done: true,
            install_done: true,
            // ...dsc 专有字段
        };
        cp.save_to(dir.path()).unwrap();
        let loaded = crate::cluster::checkpoint::ClusterCheckpoint::load_from(dir.path())
            .unwrap().unwrap();
        assert!(loaded.preflight_done, "preflight gate: 应可跳过");
    }
}
```

---

### `src/cluster/dsc/deploy.rs` (service, CRUD + remote-exec)

**Analog:** `src/cluster/deploy.rs`

**Imports pattern** (lines 1-12):
```rust
use anyhow::{Context, Result};
use std::path::Path;

use crate::common::ssh::CommandRunner;
use crate::cluster::templates::{/* dsc 专有 template 函数 */};
use crate::config::cluster::{DminitConfig, NodeConfig};

use crate::common::shell_quote;
```

**shell_quote 防注入模式** — 所有路径参数必须通过 shell_quote 包裹（deploy.rs 全文一致）:
```rust
let cmd = format!(
    "bash {} -t dmcss -dcr_ini {} -p DMCSS",
    shell_quote(&script),
    shell_quote(dmdcr_ini_path)
);
```

**dm_service_installer.sh 注册 + systemctl 启动模式**（lines 322-390 in deploy.rs）:
```rust
pub async fn register_and_start_dmwatcher_service(
    node: &NodeConfig,
    dminit: &DminitConfig,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let script = format!("{}/script/root/dm_service_installer.sh", dminit.install_path);
    let register_cmd = format!(
        "bash {} -t dmwatcher -watcher_ini {}",
        shell_quote(&script),
        shell_quote(&watcher_ini),
    );
    let (stdout, exit_code) = runner
        .exec(&register_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("执行 dm_service_installer.sh 失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "dmwatcher 服务注册失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    start_and_enable_remote_service("DmWatcherService", runner).await
}

async fn start_and_enable_remote_service(name: &str, runner: &dyn CommandRunner) -> Result<()> {
    let (stdout, exit_code) = runner
        .exec(&format!("systemctl start {}", shell_quote(name)))
        .await
        .map_err(|e| anyhow::anyhow!("启动服务 {} 失败: {}", name, e))?;
    anyhow::ensure!(exit_code == 0, "systemctl start {} 失败 (exit {}): {}", name, exit_code, String::from_utf8_lossy(&stdout));
    if let Err(e) = runner.exec(&format!("systemctl enable {}", shell_quote(name))).await {
        tracing::warn!("systemctl enable {} 失败，服务已启动但未设置开机自启: {}", name, e);
    }
    Ok(())
}
```

**DSC 专有：DMCSS 服务注册**（对应 deploy.rs 中 dmwatcher/dmmonitor 模式，参数不同）:
```rust
pub async fn register_and_start_dmcss_service(
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
    let (stdout, exit_code) = runner.exec(&cmd).await
        .map_err(|e| anyhow::anyhow!("执行 dm_service_installer.sh 失败: {}", e))?;
    anyhow::ensure!(exit_code == 0, "dmcss 服务注册失败 (exit {}): {}", exit_code, String::from_utf8_lossy(&stdout));
    start_and_enable_remote_service("DmCSSServiceDMCSS", runner).await
}
```

**disql 查询 V$INSTANCE 模式**（lines 397-445 in deploy.rs，DSC 版本检查 NORMAL 而非 PRIMARY/STANDBY）:
```rust
pub async fn verify_node_role(
    node: &NodeConfig,
    dminit: &DminitConfig,
    expected_role: NodeRole,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let cmd = format!(
        "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
        shell_quote(&dminit.install_path),
        shell_quote(&dminit.sysdba_password),
        dminit.port,
    );
    let (stdout, exit_code) = runner.exec(&cmd).await
        .map_err(|e| anyhow::anyhow!("验证节点角色 disql 执行失败: {}", e))?;
    anyhow::ensure!(exit_code == 0, "验证节点角色 disql 失败 (exit {}): {}", exit_code, String::from_utf8_lossy(&stdout));
    // DSC 版本改为检查 output.contains("OPEN") 且 output.contains("NORMAL")
}
```

**SFTP 推送配置文件模式**（lines 122-178 in deploy.rs）:
```rust
runner
    .sftp_write(&remote_path, content.as_bytes())
    .await
    .context("SFTP 上传 xxx.ini 失败")?;
```

**echo pipe 执行交互命令模式**（RESEARCH.md Pattern 2，与 deploy.rs 中 disql 模式完全相同）:
```rust
// dmasmcmd stdin pipe 模式（参照 stop_dmserver 的 echo | disql 模式）
let cmd = format!(
    "echo '{}' | {}/bin/dmasmcmd",
    cmds,
    shell_quote(install_path)
);
let (stdout, exit_code) = runner.exec(&cmd).await?;
anyhow::ensure!(exit_code == 0, "dmasmcmd 初始化失败: {}", String::from_utf8_lossy(&stdout));
```

**复制 config 目录（tar + SFTP）模式**（参照 download_backup_files/upload_backup_files，lines 222-245）:
```rust
// 在 first_node 打包：runner.exec("tar czf /tmp/dsc1_config.tar.gz -C {dir} dsc1_config")
// sftp_read 下载 tarball
// sftp_write 上传到 other_node
// runner.exec("tar xzf /tmp/dsc1_config.tar.gz -C {data_path}")
```

**测试模式** — MockRunner + assert exec_log/sftp_log（lines 521-905 in deploy.rs）:
```rust
#[tokio::test]
async fn test_register_and_start_dmcss_service() {
    let runner = MockRunner::new(vec![]);
    register_and_start_dmcss_service("/opt/dmdbms", "/tmp/dmdcr.ini", &runner).await.unwrap();
    let log = runner.exec_log();
    assert!(log.iter().any(|c| c.contains("dm_service_installer.sh") && c.contains("-t dmcss")));
    assert!(log.iter().any(|c| c.contains("systemctl start") && c.contains("DmCSSServiceDMCSS")));
}
```

---

### `src/cluster/dsc/templates.rs` (utility, transform)

**Analog:** `src/cluster/templates/dmmal_ini.rs`

**Imports pattern** (line 1):
```rust
use crate::config::cluster::{DminitConfig, NodeConfig};
```

**INI 生成函数模式**（dmmal_ini.rs lines 7-33）— 函数接受节点列表 + 配置，输出 String：
```rust
pub fn generate_dmmal_ini(nodes: &[NodeConfig], dminit: &DminitConfig, mal: &MalConfig) -> String {
    let mut out = format!(
        "MAL_CHECK_INTERVAL = {}\n...\n\n",
        mal.check_interval, /* ... */
    );
    for (i, node) in nodes.iter().enumerate() {
        out.push_str(&format_mal_inst(i, node, dminit));
    }
    out
}

fn format_mal_inst(idx: usize, node: &NodeConfig, dminit: &DminitConfig) -> String {
    format!(
        "[MAL_INST{}]\nMAL_INST_NAME = {}\nMAL_HOST = {}\nMAL_PORT = {}\n...\n\n",
        idx + 1, node.instance_name, node.host, node.mal_port,
    )
}
```

**DSC templates 对应实现**（命名和签名模式）:
```rust
// dmdcr_cfg.ini — 所有节点相同，接受 &[NodeConfig] + DSC 配置
pub fn generate_dmdcr_cfg_ini(nodes: &[NodeConfig], oguid: u32, storage: &DscStorageConfig, dminit: &DminitConfig) -> String { ... }

// dmasvrmal.ini — 所有节点相同（类似 dmmal.ini 格式）
pub fn generate_dmasvrmal_ini(nodes: &[NodeConfig]) -> String { ... }

// dmdcr.ini — 各节点不同，按 index 区分 DMDCR_SEQNO
pub fn generate_dmdcr_ini(node_index: usize, install_path: &str, dsc_conf_dir: &str, data_path: &str, instance_name: &str, storage: &DscStorageConfig) -> String { ... }

// dminit.ini — 仅 first_node 使用，PATH 使用 +DISKGROUP 语法
pub fn generate_dminit_ini(nodes: &[NodeConfig], dminit: &DminitConfig, oguid: u32, storage: &DscStorageConfig) -> String { ... }
```

**测试模式**（dmmal_ini.rs lines 35-107）— 纯单元测试，验证输出字符串包含关键字段：
```rust
#[test]
fn test_dmmal_ini_same_for_both_nodes() {
    let nodes = vec![make_primary(), make_standby()];
    let dminit = make_dminit();
    let a = generate_dmmal_ini(&nodes, &dminit, &MalConfig::default());
    assert!(a.contains("[MAL_INST1]"), "应含 [MAL_INST1]");
    assert!(a.contains("MAL_INST_NAME = DMSVR01"), "应含主节点实例名");
}
```

DSC templates 版本：
```rust
#[test]
fn test_dmdcr_cfg_ini_contains_css_grp() { /* assert contains "[GRP]\nDCR_GRP_TYPE = CSS" */ }

#[test]
fn test_dmdcr_ini_seqno_differs_per_node() {
    let ini0 = generate_dmdcr_ini(0, ...);
    let ini1 = generate_dmdcr_ini(1, ...);
    assert!(ini0.contains("DMDCR_SEQNO = 0"));
    assert!(ini1.contains("DMDCR_SEQNO = 1"));
}

#[test]
fn test_dminit_ini_asm_path_prefix() {
    let ini = generate_dminit_ini(...);
    assert!(ini.contains("SYSTEM_PATH = +DMDATA"), "+ 前缀代表 ASM 磁盘组");
}
```

---

### `src/cluster/checkpoint.rs` (modify: add DSC fields) (model, CRUD)

**Analog:** `src/cluster/checkpoint.rs` (自身，扩展模式)

**现有结构**（lines 7-19）:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClusterCheckpoint {
    #[serde(default)]
    pub preflight_done: bool,
    #[serde(default)]
    pub install_done: bool,
    #[serde(default)]
    pub primary_init_done: bool,
    #[serde(default)]
    pub backup_done: bool,
    #[serde(default)]
    pub standby_restore_done: bool,
}
```

**扩展模式** — 追加 DSC 专有字段，所有字段必须加 `#[serde(default)]`（保证向前兼容，旧 checkpoint 文件可正常反序列化）:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClusterCheckpoint {
    // ... 现有字段保持不变 ...
    // DSC 专有字段（#[serde(default)] 确保旧文件不报错）
    #[serde(default)]
    pub dsc_config_distributed: bool,
    #[serde(default)]
    pub css_asm_started: bool,
    #[serde(default)]
    pub asm_diskgroup_created: bool,
    #[serde(default)]
    pub dminit_shared_done: bool,
    #[serde(default)]
    pub config_dir_distributed: bool,
    #[serde(default)]
    pub dmserver_started: bool,
}
```

**测试模式** — 追加 roundtrip 测试验证新字段，参照 lines 74-123：
```rust
#[test]
fn test_dsc_checkpoint_roundtrip() {
    let dir = TempDir::new().unwrap();
    let cp = ClusterCheckpoint {
        dsc_config_distributed: true,
        asm_diskgroup_created: false,
        ..Default::default()
    };
    cp.save_to(dir.path()).unwrap();
    let loaded = ClusterCheckpoint::load_from(dir.path()).unwrap().unwrap();
    assert!(loaded.dsc_config_distributed);
    assert!(!loaded.asm_diskgroup_created);
}
```

---

### `src/config/cluster.rs` (modify: DscStorageConfig) (config, CRUD)

**Analog:** `src/config/cluster.rs` (自身，扩展模式)

**现有 ClusterSpecificConfig 结构**（lines 218-244）:
```rust
pub struct ClusterSpecificConfig {
    pub oguid: u32,
    #[serde(default)]
    pub nodes: Vec<NodeConfig>,
    pub shared_storage: Option<String>,  // 当前 DSC 占位字段
    // ...
}
```

**扩展模式** — 新增 `DscStorageConfig` struct，替换/扩展 `shared_storage`：
```rust
/// DSC 共享存储磁盘配置，对应 dsc.toml 中的 [dsc_storage] 段。
#[derive(Debug, Deserialize, Clone)]
pub struct DscStorageConfig {
    /// DCR 磁盘路径（块设备），如 /dev/raw/raw1
    pub dcr_disk: String,
    /// 表决磁盘路径（块设备），如 /dev/raw/raw2
    pub vote_disk: String,
    /// ASM 日志磁盘路径，如 /dev/raw/raw3（DMLOG 磁盘组）
    pub log_disk: String,
    /// ASM 数据磁盘路径，如 /dev/raw/raw4（DMDATA 磁盘组）
    pub data_disk: String,
}
```

**现有 Default + 函数模式**（lines 39-47，dmmal_ini.rs 或 cluster.rs 一致）:
```rust
impl Default for DscStorageConfig {
    fn default() -> Self {
        Self {
            dcr_disk: "/dev/raw/raw1".to_string(),
            vote_disk: "/dev/raw/raw2".to_string(),
            log_disk: "/dev/raw/raw3".to_string(),
            data_disk: "/dev/raw/raw4".to_string(),
        }
    }
}
```

**validate_dsc 扩展模式**（lines 297-308 in cluster.rs）:
```rust
fn validate_dsc(cfg: &ClusterSpecificConfig) -> Result<()> {
    check_nodes_not_empty(cfg)?;
    check_role_uniqueness(cfg)?;
    check_oguid_range(cfg)?;
    validate_dminit_config(&cfg.dminit)?;
    check_node_fields(cfg)?;
    check_instance_name_uniqueness(cfg)?;
    // 扩展：验证 dsc_storage 字段存在
    if cfg.dsc_storage.is_none() {
        bail!("配置验证失败: DSC 集群必须配置 [dsc_storage]（dcr_disk/vote_disk/log_disk/data_disk）");
    }
    Ok(())
}
```

---

## Shared Patterns

### Shell 注入防护
**Source:** `src/common/mod.rs` 中的 `shell_quote()` 函数，deploy.rs 全文使用
**Apply to:** `dsc/deploy.rs` 中所有 SSH 命令构造
```rust
use crate::common::shell_quote;

// 所有路径参数必须包裹：
let cmd = format!("{}/bin/dmasmcmd", shell_quote(install_path));
```

### CommandRunner Trait（SSH 抽象）
**Source:** `src/common/ssh/runner.rs` lines 7-27
**Apply to:** `dsc/deploy.rs` 所有函数签名
```rust
pub async fn some_dsc_fn(
    install_path: &str,
    dmdcr_ini_path: &str,
    runner: &dyn CommandRunner,  // 统一使用 trait object
) -> Result<()>
```

### anyhow::ensure! 错误处理
**Source:** `src/cluster/deploy.rs` 全文（如 lines 64-69、97-103）
**Apply to:** `dsc/deploy.rs` 所有远程命令调用
```rust
let (stdout, exit_code) = runner.exec(&cmd).await
    .map_err(|e| anyhow::anyhow!("xxx 执行失败: {}", e))?;
anyhow::ensure!(
    exit_code == 0,
    "xxx 失败 (exit {}): {}",
    exit_code,
    String::from_utf8_lossy(&stdout)
);
```

### tracing::info! 日志格式
**Source:** `src/cluster/deploy.rs`、`src/cluster/phases.rs` 全文
**Apply to:** `dsc/deploy.rs`、`dsc/mod.rs` 所有关键步骤
```rust
tracing::info!("[cluster][N/10] 步骤描述（所有节点/first_node）");
tracing::info!("[node:{:?}] 节点级操作描述", node.role);
tracing::warn!("[node:{:?}] 可恢复警告", node.role);
```

### MockRunner 测试注入
**Source:** `src/common/ssh/mock.rs`
**Apply to:** `dsc/deploy.rs`、`dsc/mod.rs` 所有 `#[cfg(test)]` 块
```rust
use crate::common::ssh::MockRunner;

// 验证 exec 调用
let runner = MockRunner::new(vec![]);
some_dsc_fn(&runner).await.unwrap();
let log = runner.exec_log();
assert!(log.iter().any(|c| c.contains("dmasmcmd")));

// 验证 SFTP 上传
let sftp_log = runner.sftp_log();
assert!(sftp_log.iter().any(|(p, _)| p.contains("dmdcr.ini")));
```

### futures::future::try_join_all 并行执行
**Source:** `src/cluster/phases.rs` lines 44-51（run_install_phase）、 lines 125-141（run_distribute_phase）
**Apply to:** `dsc/deploy.rs`、`dsc/mod.rs` 中所有节点并行步骤
```rust
let futs: Vec<_> = runners.iter().map(|(node, runner)| {
    let node = node.clone();
    let runner = Arc::clone(runner);
    async move { dsc_deploy_fn(&node, runner.as_ref()).await }
}).collect();
futures::future::try_join_all(futs).await?;
```

### #[serde(default)] 向前兼容
**Source:** `src/cluster/checkpoint.rs` lines 8-18
**Apply to:** `ClusterCheckpoint` 新增 DSC 字段、`ClusterSpecificConfig` 新增 `dsc_storage` 字段
```rust
#[serde(default)]
pub dsc_config_distributed: bool,
```

---

## No Analog Found

所有新文件均有密切匹配的模拟文件，无需纯从 RESEARCH.md 构建。

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| — | — | — | 全部有 analog |

---

## Key Pitfalls to Note for Planner

1. **run_install_phase 内嵌 dminit**：`phases::run_install_phase` 末尾自动调用 `deploy::run_dminit_remote`（phases.rs lines 52-56），DSC 中必须跳过或新建 `run_dsc_install_only_phase` 只调用 `deploy::upload_installer_and_install`，不调用 `deploy::run_dminit_remote`。
2. **dmdcr.ini DMDCR_SEQNO 必须按节点 index 区分**：在 `generate_dmdcr_ini(node_index, ...)` 中使用 `index` 作为 SEQNO，分别 SFTP 推送到各自节点，不可复用同一文件。
3. **dmasmtool 必须在 DMASM 服务启动后才能调用**：严格顺序：DMCSS start → 等待 → DMASM start → 等待 → dmasmtool。
4. **DSC V$INSTANCE 验证期望 NORMAL**：`verify_dsc_node` 检查 `output.contains("NORMAL")`，不是 PRIMARY/STANDBY（Pitfall 5 in RESEARCH.md）。
5. **dminit.ini 中 SYSTEM_PATH 必须加 + 前缀**：`+DMDATA/data` 而非 `/dev/sdc`（Pitfall 4 in RESEARCH.md）。

---

## Metadata

**Analog search scope:** `src/cluster/`, `src/config/`, `src/common/ssh/`
**Files scanned:** 16 source files
**Pattern extraction date:** 2026-06-15
