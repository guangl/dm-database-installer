---
phase: 03-cluster
plan: "01"
subsystem: config + cluster-templates
tags: [cluster, toml, config, templates, ini]
dependency_graph:
  requires: [phase-02-toml]
  provides: [ClusterConfig, NodeConfig, SshCredentials, NodeRole, load_cluster_config, validate_cluster_config, generate_dmmal_ini, generate_dmarch_ini, generate_dmwatcher_ini, generate_dm_ini_cluster_suffix]
  affects: [src/config/mod.rs, src/main.rs]
tech_stack:
  added: []
  patterns: [serde-toml-deserialize, anyhow-validate-chain, format-template-string, tdd-red-green]
key_files:
  created:
    - src/config/cluster.rs
    - src/cluster/mod.rs
    - src/cluster/templates/mod.rs
    - src/cluster/templates/dm_ini.rs
    - src/cluster/templates/dmmal_ini.rs
    - src/cluster/templates/dmarch_ini.rs
    - src/cluster/templates/dmwatcher_ini.rs
    - tests/fixtures/cluster_valid.toml
    - tests/fixtures/cluster_invalid_no_primary.toml
  modified:
    - src/config/mod.rs
    - src/main.rs
decisions:
  - "validate_cluster_config 拆为多个 private helper 函数（check_nodes_not_empty/check_role_uniqueness/check_oguid_range/check_node_fields/check_instance_name_uniqueness），保证每个函数 < 40 行"
  - "cluster::run() 保持无参数 stub 签名，Plan 03 改为接受 ClusterDeployArgs"
  - "generate_dm_ini_cluster_suffix 接受 &NodeConfig 参数但暂不使用，保留未来扩展接口"
metrics:
  duration_minutes: 5
  completed_date: "2026-06-12T12:29:38Z"
  tasks_completed: 2
  tasks_total: 2
  files_created: 9
  files_modified: 2
  tests_added: 17
---

# Phase 03 Plan 01: 集群配置 Schema + INI 模板生成器 Summary

**一行概述:** 以 serde TOML 反序列化实现 ClusterConfig/NodeConfig schema，含 8 条语义验证规则，同时生成 4 个达梦主备集群配置文件模板函数并覆盖全部 5 个关键 Pitfall。

## Tasks Completed

| Task | Name | Commit | Key Files |
|------|------|--------|-----------|
| 1 | 创建 ClusterConfig schema 与 fixtures | 722f6d4 | src/config/cluster.rs, tests/fixtures/*.toml |
| 2 | 创建 cluster 模块骨架 + 四个 INI 模板生成函数 | 2adeeb7 | src/cluster/{mod.rs,templates/*} |

## Verification Results

```
cargo build   — OK (warnings only, expected for stub phase)
cargo test    — 56 passed; 0 failed
  config::cluster::tests — 9 tests PASS
  cluster::templates — 7 tests PASS (dm_ini:1, dmmal:1, dmarch:2, dmwatcher:3)
  Phase 2 regression — 40 tests still PASS
```

## Files Created

| File | Lines | Description |
|------|-------|-------------|
| src/config/cluster.rs | 500 | ClusterConfig + validate + 9 unit tests |
| src/cluster/mod.rs | 10 | run() stub + pub mod templates |
| src/cluster/templates/mod.rs | 9 | 4 pub use re-exports |
| src/cluster/templates/dm_ini.rs | 52 | generate_dm_ini_cluster_suffix + 1 test |
| src/cluster/templates/dmmal_ini.rs | 96 | generate_dmmal_ini + 1 test (byte-equality) |
| src/cluster/templates/dmarch_ini.rs | 94 | generate_dmarch_ini + 2 tests (direction) |
| src/cluster/templates/dmwatcher_ini.rs | 124 | generate_dmwatcher_ini + 3 tests (INST_INI + OGUID) |
| tests/fixtures/cluster_valid.toml | 27 | 完整集群 TOML 示例 (2 节点) |
| tests/fixtures/cluster_invalid_no_primary.toml | 27 | 无 primary 节点的非法 fixture |

## ClusterConfig 字段最终列表

### ClusterConfig
- `cluster: ClusterSection`

### ClusterSection
- `installer_package: PathBuf`（必填，无默认）
- `oguid: u32`（默认 453331）
- `nodes: Vec<NodeConfig>`

### NodeConfig（13 字段）
- `role: NodeRole`（必填）
- `host: String`（必填）
- `port: u16`（默认 5236）
- `instance_name: String`（必填）
- `install_path: String`（默认 /opt/dmdbms）
- `data_path: String`（默认 /opt/dmdbms/data）
- `mal_port: u16`（默认 5237）
- `dw_port: u16`（默认 5238）
- `inst_dw_port: u16`（默认 5239）
- `page_size: u8`（默认 8）
- `charset: u8`（默认 0）
- `case_sensitive: bool`（默认 true）
- `extent_size: u8`（默认 16）
- `ssh: SshCredentials`（必填）

### SshCredentials
- `user: String`（必填）
- `identity_file: Option<PathBuf>`
- `password: Option<String>`（#[serde(skip_serializing)]）

## Validation Rules Implemented

| Rule | Error Message Fragment | Test |
|------|------------------------|------|
| 节点列表非空 | 集群必须至少含一个节点 | — |
| 恰好一个 primary | 必须恰好一个 primary 节点 | test_validate_rejects_no_primary |
| oguid 范围 ≤ 2147483647 | oguid 越界 | test_validate_rejects_oguid_overflow |
| port != 0 | port 无效: 0 | — |
| mal_port != port | mal_port 不能等于 port | test_validate_rejects_port_conflict |
| SSH 凭据至少一种 | 至少提供 identity_file 或 password 之一 | test_validate_rejects_missing_ssh_credentials |
| page_size ∈ {4,8,16,32} | page_size 无效 | test_validate_rejects_invalid_page_size |
| instance_name 唯一 | instance_name 重复 | test_validate_rejects_duplicate_instance_name |

## INI Pitfall 防范覆盖

| Pitfall | 防范机制 | 测试 |
|---------|---------|------|
| Pitfall 1: dmmal 主备内容不一致 | 同一函数同一参数，bytes 必然相等 | assert_eq!(a, b) |
| Pitfall 3: dmwatcher INST_INI 路径误指 | 使用 node.data_path + node.instance_name | test_dmwatcher_ini_standby_inst_ini_path_is_own |
| Pitfall 5: OGUID 主备不一致 | 接受单一 oguid 参数传入 | test_dmwatcher_ini_oguid_consistent |
| dmarch 方向（主备 ARCH_DEST 相反） | peer_instance 参数分别传备/主实例名 | test_dmarch_ini_primary/standby_dest_is_* |
| Pitfall 2（间接）: instance_name 唯一验证 | validate_cluster_config 检查重复 | test_validate_rejects_duplicate_instance_name |

## Plan 03 接管 cluster::run 入口的契约

当前签名（本 Plan）：
```rust
pub async fn run() -> Result<()> { unimplemented!("cluster::run 由 Plan 03 实现") }
```

Plan 03 改为：
```rust
pub async fn run(args: &crate::cli::ClusterDeployArgs) -> Result<()> { ... }
```

同时 Plan 03 须在 `src/cli.rs` 新增 `Commands::Cluster(ClusterArgs)` variant，并在 `main.rs` match 分支添加路由。

## Deviations from Plan

None — plan executed exactly as written.

## Known Stubs

| Stub | File | Reason |
|------|------|--------|
| `pub async fn run()` | src/cluster/mod.rs | Plan 03 实现完整编排逻辑，本 Plan 只提供模块骨架 |

## Threat Flags

None found — no new network endpoints, auth paths, file access patterns, or schema changes at trust boundaries beyond what is covered in the plan's threat model.

## Self-Check: PASSED

- [x] src/config/cluster.rs — 存在
- [x] src/cluster/mod.rs — 存在
- [x] src/cluster/templates/mod.rs — 存在
- [x] src/cluster/templates/dm_ini.rs — 存在
- [x] src/cluster/templates/dmmal_ini.rs — 存在
- [x] src/cluster/templates/dmarch_ini.rs — 存在
- [x] src/cluster/templates/dmwatcher_ini.rs — 存在
- [x] tests/fixtures/cluster_valid.toml — 存在
- [x] tests/fixtures/cluster_invalid_no_primary.toml — 存在
- [x] commit 722f6d4 (Task 1) — 存在
- [x] commit 2adeeb7 (Task 2) — 存在
- [x] 56 tests all pass — 验证
