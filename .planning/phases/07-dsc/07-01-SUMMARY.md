---
phase: 07-dsc
plan: 01
subsystem: database
tags: [rust, dsc, config, checkpoint, serde, toml]

# Dependency graph
requires:
  - phase: 05-rws
    provides: ClusterCheckpoint 结构基础（5 字段）；ClusterSpecificConfig 基础定义
  - phase: 06-status
    provides: SshCredentials.port 字段和 DminitConfig.sysdba_password 字段（Phase 6 引入，测试代码未同步）

provides:
  - DscStorageConfig struct（dcr_disk/vote_disk/log_disk/data_disk 四块设备字段，含 Default 实现）
  - ClusterSpecificConfig.dsc_storage: Option<DscStorageConfig> 字段
  - validate_dsc 校验：dsc_storage 存在性 + 四路径互不相同 + 非空
  - ClusterCheckpoint 6 个 DSC 专有字段（全部 #[serde(default)]，向前兼容旧 JSON）
  - 5 个 DSC 配置测试 + 2 个 checkpoint 测试

affects:
  - 07-02 (DSC 模板生成：使用 DscStorageConfig)
  - 07-03 (DSC 部署函数：使用 DscStorageConfig + ClusterCheckpoint DSC 字段)
  - 07-04 (DSC 入口编排：使用 ClusterCheckpoint DSC 字段控制断点续传)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "DscStorageConfig: 独立 struct 而非内联字段，与 MalConfig/WatcherConfig 模式一致"
    - "旧字段（shared_storage）保留并加 #[allow(dead_code)]，避免旧 TOML 文件解析报 unknown field"
    - "ClusterCheckpoint 向前兼容：所有新字段必须 #[serde(default)]，保证旧 JSON 文件可反序列化"
    - "validate_dsc_storage 拆分为独立函数，单一职责"

key-files:
  created: []
  modified:
    - src/config/cluster.rs
    - src/cluster/checkpoint.rs
    - src/cluster/rws/mod.rs
    - src/cluster/primary_standby/mod.rs
    - src/cluster/preflight.rs
    - src/cluster/templates/dmarch_ini.rs
    - src/cluster/templates/dmmal_ini.rs
    - src/cluster/templates/dmmonitor_ini.rs
    - src/cluster/templates/dmwatcher_ini.rs

key-decisions:
  - "保留 shared_storage 旧字段（标记 deprecated + #[allow(dead_code)]）：避免旧 dsc.toml 文件因 unknown field 报错，代价只是一个未使用字段"
  - "validate_dsc_storage 拆分为独立函数：单一职责，便于 07-03 deploy 复用磁盘校验逻辑"
  - "DscStorageConfig Default 实现使用 /dev/raw/raw1-4：与官方 DM DSC 文档默认示例一致"

patterns-established:
  - "DSC 配置字段命名：dsc_storage（段名）而非 shared_storage，与 [dsc_storage] TOML 段名一致"
  - "ClusterCheckpoint 字段顺序：通用字段先，DSC 专有字段追加到末尾，确保向前兼容"

requirements-completed:
  - DSC-01
  - DSC-02
  - DSC-03

# Metrics
duration: 7min
completed: 2026-06-15
---

# Phase 7 Plan 01: DSC 数据模型扩展 Summary

**DscStorageConfig 四块设备磁盘结构 + ClusterCheckpoint 6 个 DSC 断点字段，形成 Phase 7 后续三个计划的类型契约**

## Performance

- **Duration:** 7 min
- **Started:** 2026-06-15T03:13:43Z
- **Completed:** 2026-06-15T03:20:47Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments

- 新增 `DscStorageConfig` struct，含 dcr_disk/vote_disk/log_disk/data_disk 四个块设备路径字段和默认值（/dev/raw/raw1-4）
- `validate_dsc` 升级为校验 `[dsc_storage]` 段：存在性检查 + 四路径互不相同 + 非空检查
- `ClusterCheckpoint` 新增 6 个 DSC 专有字段（全部 `#[serde(default)]`），向前兼容旧版本 JSON 文件

## 新增/修改字段与函数清单

### src/config/cluster.rs

| 新增/修改 | 内容 |
|---------|------|
| 新增 struct | `pub struct DscStorageConfig { dcr_disk, vote_disk, log_disk, data_disk }` |
| 新增 impl | `impl Default for DscStorageConfig` → /dev/raw/raw1~raw4 |
| 修改 struct | `ClusterSpecificConfig` 新增 `pub dsc_storage: Option<DscStorageConfig>` |
| 保留 deprecated | `shared_storage: Option<String>` 标记 `#[allow(dead_code)]` 保留向后兼容 |
| 修改 fn | `validate_dsc` → 改为检查 `dsc_storage.is_none()` |
| 新增 fn | `validate_dsc_storage` → 非空检查 + HashSet 去重校验 |

### src/cluster/checkpoint.rs

| 新增字段 | 语义 |
|---------|------|
| `dsc_config_distributed` | 配置文件已推送到所有节点 |
| `css_asm_started` | 所有节点 DMCSS+DMASM 服务已启动 |
| `asm_diskgroup_created` | first_node 磁盘组初始化完成 |
| `dminit_shared_done` | first_node 共享存储 dminit 完成 |
| `config_dir_distributed` | dscN_config 目录已分发到其余节点 |
| `dmserver_started` | 所有节点 dmserver 已启动并验证 |

## 测试覆盖矩阵

| 测试名 | 覆盖内容 | 结果 |
|--------|---------|------|
| test_dsc_storage_config_default_values | DscStorageConfig::default() 默认值 | PASS |
| test_dsc_storage_config_deserializes | TOML 反序列化四个磁盘字段 | PASS |
| test_dsc_requires_dsc_storage | 缺少 [dsc_storage] 时报错含 "dsc_storage" | PASS |
| test_dsc_accepts_dsc_storage | 完整 [dsc_storage] 配置通过校验 | PASS |
| test_dsc_storage_disks_must_be_distinct | 重复磁盘路径被拒绝 | PASS |
| test_dsc_checkpoint_roundtrip | 6 个 DSC 字段 save+load roundtrip | PASS |
| test_old_checkpoint_file_still_loads | 旧 JSON（5 字段）仍可加载，DSC 字段默认 false | PASS |
| test_checkpoint_gate_skips_done_phases (rws) | RWS 测试同步更新新字段后仍通过 | PASS |

## 旧 dsc.toml → 新 dsc.toml 迁移说明

**旧格式（已废弃）：**
```toml
oguid = 453331
shared_storage = "/dev/sdc"

[[nodes]]
...
```

**新格式（推荐）：**
```toml
oguid = 453331

[dsc_storage]
dcr_disk = "/dev/raw/raw1"
vote_disk = "/dev/raw/raw2"
log_disk = "/dev/raw/raw3"
data_disk = "/dev/raw/raw4"

[[nodes]]
...
```

旧格式中的 `shared_storage` 字段将被忽略（保留字段定义以防 TOML 解析报 unknown field），DSC 部署逻辑将只读 `dsc_storage`。

## Task Commits

TDD 每个任务含多次提交（test → feat）：

1. **Task 1 RED: 扩展 DscStorageConfig + validate_dsc（失败测试）** - `baaed9e` (test)
2. **Task 1 GREEN: 实现 DscStorageConfig 和 validate_dsc** - `07d067d` (feat)
3. **Task 2 RED: ClusterCheckpoint DSC 字段（失败测试）** - `b0d07cc` (test)
4. **Task 2 GREEN: ClusterCheckpoint 6 个 DSC 字段实现** - `7588a77` (feat)

## Files Created/Modified

- `src/config/cluster.rs` — 新增 DscStorageConfig struct；ClusterSpecificConfig 新增 dsc_storage 字段；validate_dsc 升级
- `src/cluster/checkpoint.rs` — ClusterCheckpoint 新增 6 个 DSC 字段
- `src/cluster/rws/mod.rs` — 同步更新 test_checkpoint_gate_skips_done_phases 显式构造
- `src/cluster/primary_standby/mod.rs` — [Rule 3] 补齐 dsc_storage: None 和测试构造字段
- `src/cluster/preflight.rs` — [Rule 3] 补齐测试构造中缺失的 SshCredentials.port 和 DminitConfig.sysdba_password
- `src/cluster/templates/dmarch_ini.rs` — [Rule 3] 同上
- `src/cluster/templates/dmmal_ini.rs` — [Rule 3] 同上
- `src/cluster/templates/dmmonitor_ini.rs` — [Rule 3] 补齐 SshCredentials.port
- `src/cluster/templates/dmwatcher_ini.rs` — [Rule 3] 补齐 SshCredentials.port 和 DminitConfig.sysdba_password

## Decisions Made

- 保留 `shared_storage` 旧字段（标记 deprecated + `#[allow(dead_code)]`）：避免旧 dsc.toml 文件因 unknown field 解析失败
- `validate_dsc_storage` 拆分为独立函数：便于 07-03 deploy 复用磁盘校验逻辑
- `DscStorageConfig::default()` 使用 `/dev/raw/raw1-4`：与 DM 官方文档 raw 设备命名惯例一致

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] 修复预存在的测试代码编译错误（SshCredentials.port + DminitConfig.sysdba_password）**
- **Found during:** Task 1 GREEN 阶段（运行 cargo test 时）
- **Issue:** Phase 6 新增 `SshCredentials.port` 和 `DminitConfig.sysdba_password` 字段后，多个测试文件中的显式构造未同步补字段，导致 `cargo test --bin dm-installer` 编译失败
- **Fix:** 在 7 个测试文件的显式构造中补齐 `port: 22` 和 `sysdba_password: "SYSDBA".to_string()`
- **Files modified:** src/cluster/preflight.rs, src/cluster/primary_standby/mod.rs, src/cluster/templates/dmarch_ini.rs, src/cluster/templates/dmmal_ini.rs, src/cluster/templates/dmmonitor_ini.rs, src/cluster/templates/dmwatcher_ini.rs
- **Verification:** `cargo test --bin dm-installer` 通过 188 个测试
- **Committed in:** `07d067d` (Task 1 feat 提交，与 DscStorageConfig 实现同批)

---

**Total deviations:** 1 auto-fixed ([Rule 3 - Blocking])
**Impact on plan:** 修复为必要的阻塞性问题，不涉及业务逻辑变更，无范围蔓延。

## Issues Encountered

`cargo test` 与 `cargo build` 编译目标不同（`--bin` vs `--lib`），项目为纯 binary crate，测试只能通过 `cargo test --bin dm-installer` 运行。

## Next Phase Readiness

- 07-02 DSC 模板生成：可直接 `use crate::config::cluster::DscStorageConfig` 生成 dmdcr_cfg.ini/dmasvrmal.ini/dmdcr.ini/dminit.ini
- 07-03 DSC 部署函数：可直接使用 `ClusterCheckpoint::dsc_config_distributed` 等字段控制断点续传
- 07-04 DSC 入口编排：类型契约已稳定，可并行/串行开发

---
*Phase: 07-dsc*
*Completed: 2026-06-15*
