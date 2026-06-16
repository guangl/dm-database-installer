# Phase 5: RWS 读写分离集群 - Pattern Map

**Mapped:** 2026-06-14
**Files analyzed:** 4
**Analogs found:** 4 / 4

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/cluster/checkpoint.rs` | utility/state | file-I/O | `src/standalone/checkpoint.rs` | exact |
| `src/cluster/phases.rs` | service | request-response (async poll) | `src/cluster/deploy.rs:395-436` (verify_node_role) + `src/cluster/deploy.rs:486-504` (configure_sqllog retry) | role-match |
| `src/cluster/rws/mod.rs` | controller | CRUD/orchestration | 自身现有结构（run_with_runners） | self-modify |
| `src/cluster/mod.rs` | config/module | — | `src/standalone/mod.rs` (pub mod checkpoint) | role-match |

---

## Pattern Assignments

### `src/cluster/checkpoint.rs` (utility, file-I/O) — NEW

**Analog:** `src/standalone/checkpoint.rs`

**Imports pattern** (lines 1-4):
```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
```

**File name constant** (line 5):
```rust
const FILE_NAME: &str = "dm_cluster_checkpoint.json";
```

**Struct pattern** (lines 7-17):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub install_path: String,
    pub sysdba_pwd: String,
    // ...
    #[serde(default)]
    pub uploaded: bool,
    pub installed: bool,
}
```
集群版差异：字段全为布尔标志（无 install_path 匹配键），使用 `Default` derive，结构体命名为 `ClusterCheckpoint`：
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

**save_to core pattern** (lines 39-45):
```rust
pub(crate) fn save_to(&self, dir: &Path) -> Result<()> {
    let path = dir.join(FILE_NAME);
    let content = serde_json::to_string_pretty(self)?;
    std::fs::write(&path, content)?;
    tracing::debug!("检查点已保存: {}", path.display());
    Ok(())
}
```

**remove_from pattern** (lines 47-54):
```rust
pub(crate) fn remove_from(dir: &Path) -> Result<()> {
    let path = dir.join(FILE_NAME);
    if path.exists() {
        std::fs::remove_file(&path)?;
        tracing::debug!("检查点已删除");
    }
    Ok(())
}
```

**load_from pattern** (lines 62-85) — corrupt file handling 关键：
```rust
pub(crate) fn load_from(dir: &Path, install_path: &str) -> Result<Option<Checkpoint>> {
    let path = dir.join(FILE_NAME);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let cp: Checkpoint = match serde_json::from_str(&content) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("检查点文件格式错误，忽略: {}", e);
            return Ok(None);
        }
    };
    // ... 匹配键检查（集群版无此逻辑）
    println!("[续] 检测到检查点，从上次进度继续安装");
    Ok(Some(cp))
}
```
集群版 load_from 不需要 install_path 参数，签名为 `pub(crate) fn load_from(dir: &Path) -> Result<Option<ClusterCheckpoint>>`，省去匹配键检查。

**公开 cwd 代理方法 pattern** (lines 31-37, 58-59):
```rust
pub fn save(&self) -> Result<()> {
    self.save_to(&cwd())
}
pub fn remove() -> Result<()> {
    Self::remove_from(&cwd())
}
pub fn load() -> Result<Option<Self>> {
    Self::load_from(&cwd())
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
```

**Test pattern** (lines 91-141) — 使用 `tempfile::TempDir` + `_to/_from` 变体：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_roundtrip_save_load() {
        let dir = TempDir::new().unwrap();
        let mut cp = make_cp("/opt/dmdbms");
        cp.installed = true;
        cp.save_to(dir.path()).unwrap();
        let loaded = load_from(dir.path(), "/opt/dmdbms").unwrap().unwrap();
        assert!(loaded.installed);
    }

    #[test]
    fn test_load_returns_none_when_no_file() { /* ... */ }

    #[test]
    fn test_remove_deletes_file() { /* ... */ }

    #[test]
    fn test_load_ignores_corrupt_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "not json").unwrap();
        assert!(ClusterCheckpoint::load_from(dir.path()).unwrap().is_none());
    }
}
```

---

### `src/cluster/phases.rs` — MODIFY: 追加 `run_read_routing_phase` + `wait_for_standby_open`

**Analog 1:** `src/cluster/phases.rs` 现有 `run_verify_phase`（lines 248-263）— 遍历 runners + filter + async 调用

**run_verify_phase 遍历模式** (lines 248-263):
```rust
pub async fn run_verify_phase(runners: &Runners, dminit: &DminitConfig) -> Result<()> {
    use crate::config::cluster::NodeRole;
    tracing::info!("[cluster][11/11] 验证主备角色状态");
    let futs: Vec<_> = runners
        .iter()
        .filter(|(n, _)| matches!(n.role, NodeRole::Primary | NodeRole::Standby))
        .map(|(node, runner)| {
            let node = node.clone();
            let runner = Arc::clone(runner);
            let dminit = dminit.clone();
            async move { deploy::verify_node_role(&node, &dminit, node.role, runner.as_ref()).await }
        })
        .collect();
    futures::future::try_join_all(futs).await?;
    Ok(())
}
```
新函数 `run_read_routing_phase` 用 `.filter()` + 串行 `for` 循环（备节点通常只有 1 个），不用 `try_join_all`。

**Analog 2:** `src/cluster/deploy.rs:486-504` — `configure_sqllog` retry loop（disql 命令 + tokio::time::sleep + 计数退出）

**Poll retry 模式** (lines 486-504):
```rust
for attempt in 1..=6u32 {
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("disql 执行失败: {}", e))?;
    if exit_code == 0 {
        tracing::info!("[node:{:?}] SQL 日志配置完成", node.role);
        return Ok(());
    }
    let output = String::from_utf8_lossy(&stdout);
    if attempt < 6 {
        tracing::warn!(
            "[node:{:?}] SQL 日志配置失败，数据库可能尚未 open（{}/6）: {}",
            node.role, attempt, output.trim()
        );
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    } else {
        anyhow::bail!("SQL 日志配置失败 (exit {}): {}", exit_code, output);
    }
}
```
新函数参数：最多 `24u32` 次（vs 6），检查输出内容（`output.contains("OPEN") && output.contains("STANDBY")`），超时消息含节点 host。

**Analog 3:** `src/cluster/deploy.rs:401-405` — disql V$INSTANCE 命令格式

**disql 命令构建模式** (lines 401-405):
```rust
let cmd = format!(
    "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/SYSDBA@localhost:{}",
    shell_quote(&dminit.install_path),
    dminit.port,
);
```
需 `use crate::common::shell_quote;` — 与 deploy.rs 第 12 行相同 import。

**新函数签名（D-11 锁定）：**
```rust
pub async fn run_read_routing_phase(
    specific: &ClusterSpecificConfig,
    runners: &Runners,
    dminit: &DminitConfig,
) -> Result<()>
```

**辅助函数签名（40 行限制强制拆分）：**
```rust
async fn wait_for_standby_open(
    node: &NodeConfig,
    dminit: &DminitConfig,
    runner: &dyn ssh::CommandRunner,
) -> Result<()>
```

**Tracing 标志：** 用 `[cluster][12/12]`（现有最高为 `[cluster][11/11]`，见 run_verify_phase 第 250 行）。

**MockRunner 测试预设模式** (`src/common/ssh/mock.rs` lines 22-34, `src/cluster/deploy.rs:843-849`):
```rust
// happy path：预设 OPEN STANDBY 输出
let runner = MockRunner::new(vec![(
    "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
    0,
    b"STATUS$   MODE$\nOPEN      STANDBY\n".to_vec(),
)]);
// timeout path：不预设响应，非 strict 模式默认返回空 stdout (exit 0)
// 空输出不含 "OPEN"，触发重试直至超时
// 注意：CI 中需将 MAX_RETRIES 提取为常量并注入小值，避免等待 120s
let runner = MockRunner::new(vec![]);  // 严格用 new_strict 防止未预期命令通过
```

---

### `src/cluster/rws/mod.rs` — MODIFY: checkpoint gate + 替换 TODO:50

**Analog:** 自身现有 `run_with_runners` 结构（lines 30-53）

**现有编排结构** (lines 38-53) — 注意 TODO 位置在第 50 行：
```rust
pub async fn run_with_runners(
    common: CommonConfig,
    specific: ClusterSpecificConfig,
    runners: phases::Runners,
    health_check_fn: impl Fn(...) + Send + Sync,
) -> Result<()> {
    let dminit = specific.dminit.clone();
    phases::run_preflight(&runners, &dminit).await?;
    phases::run_install_phase(&common, &runners, &dminit).await?;
    phases::run_primary_init_phase(&runners, &health_check_fn, &dminit).await?;
    phases::run_backup_phase(&runners, &dminit).await?;
    phases::run_standby_restore_phase(&runners, &dminit).await?;
    phases::run_distribute_phase(&specific, &runners, &dminit).await?;
    phases::run_startup_phase(&specific, &runners, &health_check_fn, &dminit).await?;
    phases::run_watcher_phase(&runners, &dminit).await?;
    phases::run_monitor_phase(&specific, &runners, &dminit).await?;
    phases::run_sqllog_phase(&specific, &runners, &dminit).await?;
    phases::run_verify_phase(&runners, &dminit).await?;
    // TODO: run_read_routing_phase — 配置读写分离路由规则   ← 第 50 行，替换此处
    tracing::info!("集群部署完成");
    Ok(())
}
```

**修改后结构** — checkpoint gate 嵌入 + TODO 替换：
```rust
pub async fn run_with_runners(...) -> Result<()> {
    let dminit = specific.dminit.clone();
    let mut cp = crate::cluster::checkpoint::ClusterCheckpoint::load()?
        .unwrap_or_default();

    // checkpoint gate 模式（5 个高代价 phase）
    if !cp.preflight_done {
        phases::run_preflight(&runners, &dminit).await?;
        cp.preflight_done = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过预检查（checkpoint）");
    }
    // ... install / primary_init / backup / standby_restore 同模式 ...

    // standby_restore_done 之后不打点，直接执行
    phases::run_distribute_phase(&specific, &runners, &dminit).await?;
    phases::run_startup_phase(&specific, &runners, &health_check_fn, &dminit).await?;
    phases::run_watcher_phase(&runners, &dminit).await?;
    phases::run_monitor_phase(&specific, &runners, &dminit).await?;
    phases::run_sqllog_phase(&specific, &runners, &dminit).await?;
    phases::run_verify_phase(&runners, &dminit).await?;
    phases::run_read_routing_phase(&specific, &runners, &dminit).await?;  // 替换 TODO

    crate::cluster::checkpoint::ClusterCheckpoint::remove()?;
    tracing::info!("集群部署完成");
    Ok(())
}
```

**checkpoint gate 单元 pattern** — 每个 gate 块约 6 行，5 个 gate 共 30 行，加上后续调用，总函数接近 40 行上限。若超出需提取 `run_checkpointed_phases` 辅助函数。

---

### `src/cluster/mod.rs` — MODIFY: 添加 `pub mod checkpoint`

**Analog:** `src/standalone/mod.rs`（推断；standalone 有 `pub mod checkpoint` 声明）

**现有 mod.rs 结构** (lines 1-13):
```rust
pub mod deploy;
pub mod dpc;
pub mod dsc;
pub mod health;
pub mod phases;
pub mod preflight;
pub mod primary_standby;
pub mod rws;
pub mod templates;
```

**修改：** 在现有 `pub mod` 列表中按字母序插入：
```rust
pub mod checkpoint;   // 新增，位于 deploy 之后或 dpc 之前
```

---

## Shared Patterns

### disql 命令构建（防注入）
**Source:** `src/cluster/deploy.rs` line 12 + lines 401-405
**Apply to:** `phases.rs` 中的 `wait_for_standby_open`
```rust
use crate::common::shell_quote;

let cmd = format!(
    "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/SYSDBA@localhost:{}",
    shell_quote(&dminit.install_path),
    dminit.port,
);
```

### Tokio 异步重试循环
**Source:** `src/cluster/deploy.rs` lines 486-504 (`configure_sqllog`)
**Apply to:** `phases.rs` 中的 `wait_for_standby_open`
```rust
tokio::time::sleep(std::time::Duration::from_secs(5)).await;
```

### Tracing 日志格式
**Source:** `src/cluster/phases.rs` lines 11-250（所有现有 phase 函数）
**Apply to:** `run_read_routing_phase` 和 `wait_for_standby_open`
```rust
tracing::info!("[cluster][12/12] ...");
tracing::warn!("[node:{:?}] ...", node.role);
tracing::info!("[node:{:?}] ...", node.role);
```

### anyhow 错误构建
**Source:** `src/cluster/phases.rs` lines 19, 55-56 等
**Apply to:** `wait_for_standby_open` 超时路径
```rust
// 超时
anyhow::bail!("备节点 {} 未在 120s 内达到 STATUS$=OPEN MODE$=STANDBY", node.host);
// exec 失败包装
.map_err(|e| anyhow::anyhow!("poll V$INSTANCE 失败: {}", e))?;
```

### serde_json checkpoint roundtrip
**Source:** `src/standalone/checkpoint.rs` lines 39-85
**Apply to:** `src/cluster/checkpoint.rs` 全部 save/load/remove 方法
```rust
// save
let content = serde_json::to_string_pretty(self)?;
std::fs::write(&path, content)?;
// load（corrupt file 静默忽略）
match serde_json::from_str(&content) {
    Ok(c) => Ok(Some(c)),
    Err(e) => { tracing::warn!("检查点文件格式错误，忽略: {}", e); Ok(None) }
}
```

---

## No Analog Found

所有 4 个文件均有对应 analog，无此类情况。

---

## Critical Anti-Patterns

以下模式不得在新代码中出现：

| 反模式 | 正确做法 | 来源 |
|--------|----------|------|
| 在 `run_read_routing_phase` 中调用 `deploy::configure_read_only_standby()` | dmwatcher 自动转换，不执行额外 SQL（D-06） | CONTEXT.md |
| 用 `deploy::verify_node_role` 替代 `wait_for_standby_open` | `verify_node_role` 不检查 `STATUS$=OPEN`，也无重试（deploy.rs:427-433） | RESEARCH.md Pitfall 1 |
| 在 `phases.rs` 函数内部调用 `cp.save()` | checkpoint gate 仅在 `rws/mod.rs` 管理（D-03） | CONTEXT.md |
| `run_read_routing_phase` 函数体超过 40 行 | poll 逻辑必须拆入 `wait_for_standby_open`（CLAUDE.md 约束） | CLAUDE.md |
| MockRunner 空响应用于超时测试且不控制等待时间 | 提取 `MAX_RETRIES/POLL_INTERVAL_SECS` 常量便于测试注入 | RESEARCH.md Pitfall 2 |

---

## Metadata

**Analog search scope:** `src/standalone/`, `src/cluster/`, `src/common/ssh/`
**Files scanned:** 7 (`checkpoint.rs`, `phases.rs`, `rws/mod.rs`, `mod.rs`, `deploy.rs`, `mock.rs`, `cluster.rs`)
**Pattern extraction date:** 2026-06-14
