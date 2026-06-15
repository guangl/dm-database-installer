---
phase: 07-dsc
plan: 02
subsystem: cluster/dsc
tags: [rust, dsc, templates, ini, tdd]

# Dependency graph
requires:
  - phase: 07-01
    provides: DscStorageConfig struct（dcr_disk/vote_disk/log_disk/data_disk）；NodeConfig.instance_name；DminitConfig.sysdba_password

provides:
  - generate_dmdcr_cfg_ini: 含 CSS/ASM/DB 三个 [GRP] 段的 dmdcr_cfg.ini 生成函数
  - generate_dmasvrmal_ini: N 个 [MAL_INSTn] 段的 dmasvrmal.ini 生成函数
  - generate_dmdcr_ini: 按节点索引设置 DMDCR_SEQNO 的 dmdcr.ini 生成函数（Pitfall 3 防御）
  - generate_dminit_ini: SYSTEM_PATH/LOG_PATH 强制 + 前缀的 dminit.ini 生成函数（Pitfall 4 防御）
  - 16 个单元测试覆盖全部函数行为

affects:
  - 07-03 (DSC 部署函数：通过 crate::cluster::dsc::templates:: 引用 4 个函数)
  - 07-04 (DSC 入口编排：间接通过 deploy.rs 消费)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "GRP 嵌套结构：dmdcr_cfg.ini 用 [GRP] + [EPn] 两层嵌套，区别于 dmmal.ini 的单层 [MAL_INSTn]"
    - "私有辅助函数（format_css_grp/format_asm_grp/format_db_grp）：把每段 GRP 拆开，保持 generate 主函数 < 20 行"
    - "#[allow(dead_code)] 于文件头：bin crate 中 pub 函数不自动免除 dead_code 警告，等 Plan 03 引用后移除"
    - "Rust format! 具名参数（{dcr_disk}、{node_index}）：generate_dmdcr_ini 中 6 个参数通过具名格式提高可读性"

key-files:
  created:
    - src/cluster/dsc/templates.rs
  modified:
    - src/cluster/dsc/mod.rs

key-decisions:
  - "4 个函数签名与 PATTERNS.md 一致：plan 03 可直接 use crate::cluster::dsc::templates::{...} 导入"
  - "不在 mod.rs 用 pub use 重导出：避免二次 unused_import 警告，Plan 03 按模块路径导入即可"
  - "#[allow(dead_code)] 而非 #[allow(unused)]：仅抑制 dead_code 类别，不影响其他 lint"
  - "dminit.ini 中 SYSTEM_PATH/LOG_PATH 硬编码 +DMDATA/+DMLOG 磁盘组名：不读取 storage.data_disk 块设备路径，与 RESEARCH.md Pitfall 4 一致"

# Metrics
metrics:
  completed: "2026-06-15"
  tasks_completed: 2
  tests_added: 16
  files_created: 1
  files_modified: 1
---

# Phase 07 Plan 02: DSC INI 模板生成函数 Summary

**One-liner:** 纯函数 4 个 DSC INI 生成器（dmdcr_cfg/dmasvrmal/dmdcr/dminit），16 个单元测试验证端口序列、SEQNO 区分和 ASM +前缀

## 4 个函数签名与输出示例

### generate_dmdcr_cfg_ini

```rust
pub fn generate_dmdcr_cfg_ini(
    nodes: &[NodeConfig], oguid: u32,
    storage: &DscStorageConfig, dminit: &DminitConfig
) -> String
```

输出（节选）：
```ini
DCR_N_GRP = 3
DCR_VTD_PATH = /dev/raw/raw2
DCR_OGUID = 63635

[GRP]
DCR_GRP_TYPE = CSS
DCR_GRP_N_EP = 2
  [EP0]
  DCR_EP_HOST = 192.168.1.10
  DCR_EP_PORT = 9341
```

### generate_dmasvrmal_ini

```rust
pub fn generate_dmasvrmal_ini(nodes: &[NodeConfig]) -> String
```

输出（节选）：
```ini
[MAL_INST0]
MAL_INST_NAME = DSC0
MAL_HOST = 192.168.1.10
MAL_PORT = 9349
```

### generate_dmdcr_ini

```rust
pub fn generate_dmdcr_ini(
    node_index: usize, install_path: &str, dsc_conf_dir: &str,
    data_path: &str, instance_name: &str, storage: &DscStorageConfig
) -> String
```

输出（节选）：
```ini
DMDCR_PATH = /dev/raw/raw1
DMDCR_SEQNO = 0
DMDCR_ASM_STARTUP_CMD = /opt/dmdbms/bin/dmasmsvr DCR_INI=/opt/dmdbms/dsc_conf/dmdcr.ini
```

### generate_dminit_ini

```rust
pub fn generate_dminit_ini(
    nodes: &[NodeConfig], dminit: &DminitConfig,
    oguid: u32, storage: &DscStorageConfig
) -> String
```

输出（节选）：
```ini
SYSTEM_PATH = +DMDATA/data

[DSC0]
LOG_PATH = +DMLOG/log/dsc0_log01.log
```

## 测试矩阵

| 函数 | 测试名 | 验证内容 |
|------|--------|----------|
| generate_dmdcr_cfg_ini | test_dmdcr_cfg_ini_contains_three_grps | CSS/ASM/DB 三段均存在 |
| generate_dmdcr_cfg_ini | test_dmdcr_cfg_ini_n_grp_and_oguid | DCR_N_GRP=3、OGUID、VTD_PATH |
| generate_dmdcr_cfg_ini | test_dmdcr_cfg_ini_each_grp_has_n_ep | 三段均含 DCR_GRP_N_EP=2 |
| generate_dmdcr_cfg_ini | test_dmdcr_cfg_ini_css_ports | CSS 端口 9341/9343 |
| generate_dmdcr_cfg_ini | test_dmdcr_cfg_ini_asm_ports_and_shmkey | ASM 端口 9349/9351、SHMKEY |
| generate_dmdcr_cfg_ini | test_dmdcr_cfg_ini_db_ports | DB 端口 = dminit.port + index |
| generate_dmasvrmal_ini | test_dmasvrmal_ini_contains_inst_blocks | [MAL_INST0]/[MAL_INST1] 存在 |
| generate_dmasvrmal_ini | test_dmasvrmal_ini_inst_name_matches_node | MAL_INST_NAME = DSC0/DSC1 |
| generate_dmasvrmal_ini | test_dmasvrmal_ini_port_matches_asm_port | MAL_PORT = 9349/9351 |
| generate_dmdcr_ini | test_dmdcr_ini_seqno_differs_per_node | SEQNO=0 vs SEQNO=1（Pitfall 3） |
| generate_dmdcr_ini | test_dmdcr_ini_paths_and_intervals | DCR_PATH、MAL_PATH、重启间隔 |
| generate_dmdcr_ini | test_dmdcr_ini_startup_cmds_use_install_path | 启动命令含正确 install_path |
| generate_dminit_ini | test_dminit_ini_asm_path_prefix | +DMDATA/+DMLOG 前缀（Pitfall 4） |
| generate_dminit_ini | test_dminit_ini_per_node_blocks | [DSC0]/[DSC1]、PORT_NUM 正确 |
| generate_dminit_ini | test_dminit_ini_config_path_per_node | dsc0_config/dsc1_config |
| generate_dminit_ini | test_dminit_ini_sysdba_pwd_from_config | SYSDBA_PWD 来自配置 |

## 与 dmmal_ini.rs analog 的差异

dmmal_ini.rs：
- 单层 `[MAL_INSTn]` 结构，所有节点共用同一文件
- 函数签名含 `MalConfig` 参数（全局 MAL 参数）

dsc/templates.rs：
- **dmdcr_cfg.ini**：三层嵌套 `[GRP] → [EPn]`，每段 GRP 端口序列不同，需要私有辅助函数拆分
- **dmdcr.ini**：按节点索引生成不同文件（SEQNO 区分），不是所有节点共用
- **dminit.ini**：SYSTEM_PATH/LOG_PATH 使用 ASM 磁盘组名语法（+前缀），不能用 DscStorageConfig 的块设备路径

## 对 Plan 03 的导出建议

Plan 03 (dsc/deploy.rs) 可直接按模块路径导入：

```rust
use crate::cluster::dsc::templates::{
    generate_dmdcr_cfg_ini,
    generate_dmasvrmal_ini,
    generate_dmdcr_ini,
    generate_dminit_ini,
};
```

**不建议**在 `dsc/mod.rs` 层 `pub use` 重导出（会在 bin crate 中产生额外的 unused_import 警告）。Plan 03 实现后可移除 templates.rs 头部的 `#[allow(dead_code)]`。

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical] 添加 #[allow(dead_code)] 消除编译 warning**
- **Found during:** Task 2（GREEN 阶段 cargo build 验证）
- **Issue:** bin crate 中 pub 函数无引用时仍报 dead_code warning，违反验收标准"无 warning"
- **Fix:** 在 templates.rs 头部添加 `#![allow(dead_code)]`，明确注释说明 Plan 03 引用后移除
- **Files modified:** src/cluster/dsc/templates.rs
- **Commit:** de1f243

## TDD Gate Compliance

| 阶段 | 提交 | 验证 |
|------|------|------|
| RED | b9f8e0e | 16 个测试，全部 panic（todo!() stub）|
| GREEN | de1f243 | 16 个测试，全部通过 |
| REFACTOR | 无 | 代码结构清晰，无需重构 |

## Self-Check: PASSED

- src/cluster/dsc/templates.rs 存在: FOUND
- src/cluster/dsc/mod.rs 含 pub mod templates: FOUND
- 提交 b9f8e0e (RED) 存在: FOUND
- 提交 de1f243 (GREEN) 存在: FOUND
- 16 个测试全部通过: VERIFIED
- cargo build 无 warning: VERIFIED
