---
phase: 07-dsc
plan: 04
subsystem: cluster/dsc
tags: [rust, dsc, orchestration, checkpoint, tdd, mockrunner]

# Dependency graph
requires:
  - phase: 07-03
    provides: 10 个 pub async deploy 函数（run_dsc_install_only/distribute_dsc_configs/register_and_start_dmcss_service/register_and_start_dmasm_service/register_and_start_dmserver_service/run_dmasmcmd_init/run_dmasmtool_create_diskgroups/run_dminit_shared/distribute_config_dir/verify_dsc_node）
  - phase: 07-01
    provides: ClusterCheckpoint 8 个 DSC gate 字段；DscStorageConfig；NodeRole::Primary

provides:
  - src/cluster/dsc/mod.rs::run() — SSH 建立后委托 run_with_runners
  - src/cluster/dsc/mod.rs::run_with_runners() — 8 个 checkpoint gate 顺序编排
  - 5 个集成单元测试覆盖 gate 跳过/调用顺序/first_node 角色/全 standby 报错/中断 checkpoint 保存

affects:
  - Phase 8+（真实 DSC 硬件环境验证）

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "8 个 gate 展开在 run_with_runners 中（不用 RWS 的双层 run_early_checkpoints/run_init_restore_checkpoints 封装）"
    - "CWD_LOCK 静态 Mutex：串行化需要 set_current_dir 的测试，避免并发竞争"
    - "make_runner_with_preflight：统一预设 preflight 三项命令响应（sudo/ss/df），避免 df 解析失败"
    - "verify_dsc_node 预设：echo 'SELECT STATUS$ 前缀匹配，返回含 OPEN 的 stdout"

key-files:
  created: []
  modified:
    - src/cluster/dsc/mod.rs

key-decisions:
  - "不调用 phases::run_install_phase（因其内嵌 dminit），改用 run_dsc_install_all_nodes 自组装（Pitfall 1）"
  - "8 个 gate 直接展开在 run_with_runners 中，避免 RWS 模式的双层封装复杂度"
  - "CWD_LOCK + unwrap_or_else(|e| e.into_inner()) 处理中毒：Mutex 中毒后仍可串行化测试，无需 serial_test crate"
  - "health_check_fn 对 DMASM 端口 9349 轮询（非 DminitConfig 字段）：Gate 4 完成前确保 dmasmtool 可安全执行（Pitfall 2 防御）"

requirements-completed:
  - DSC-01
  - DSC-02
  - DSC-03

# Metrics
duration: 25min
completed: 2026-06-15
---

# Phase 7 Plan 04: DSC 入口编排 Summary

**完整实现 `dsc::run()` + `run_with_runners()`，8 个 checkpoint gate 覆盖 DSC 全生命周期，5 个集成单元测试验证 gate 跳过逻辑与 first_node 角色分离**

## Performance

- **Duration:** 25 min
- **Started:** 2026-06-15T05:18:00Z
- **Completed:** 2026-06-15T05:43:37Z
- **Tasks:** 1（Task 2 为人工验证 checkpoint，待用户触发）
- **Files modified:** 1（src/cluster/dsc/mod.rs 从 stub 完整重写）

## 流程图：10 个阶段对应函数调用与 checkpoint gate

```
run()
 └── SSH 建立 × N 节点
 └── run_with_runners()
      │
      ├── Gate 1: preflight_done
      │   └── phases::run_preflight(&runners, dminit)
      │
      ├── Gate 2: install_done
      │   └── run_dsc_install_all_nodes(common, runners, dminit)
      │       └── deploy::run_dsc_install_only × N（并行）
      │
      ├── Gate 3: dsc_config_distributed
      │   └── run_distribute_dsc_configs_all_nodes(runners, dminit, oguid, storage)
      │       └── deploy::distribute_dsc_configs × N（并行）
      │
      ├── Gate 4: css_asm_started
      │   └── run_start_css_asm_all_nodes(runners, dminit, health_check_fn)
      │       ├── deploy::register_and_start_dmcss_service × N（并行）
      │       ├── deploy::register_and_start_dmasm_service × N（并行）
      │       └── health_check_fn(:9349) × N（等待 DMASM 端口就绪）
      │
      ├── Gate 5: asm_diskgroup_created
      │   └── run_asm_init_first_node(runners, dminit, storage)
      │       ├── deploy::run_dmasmcmd_init（仅 first_node）
      │       └── deploy::run_dmasmtool_create_diskgroups（仅 first_node）
      │
      ├── Gate 6: dminit_shared_done
      │   └── run_dminit_shared_first_node(runners, all_nodes, dminit, oguid, storage)
      │       └── deploy::run_dminit_shared（仅 first_node）
      │
      ├── Gate 7: config_dir_distributed
      │   └── run_distribute_config_dirs(runners, dminit)
      │       └── deploy::distribute_config_dir × (N-1)（串行，first_runner → other_runner）
      │
      └── Gate 8: dmserver_started
          └── run_start_and_verify_dmserver_all_nodes(runners, dminit, health_check_fn)
              ├── deploy::register_and_start_dmserver_service（first_node 先）
              ├── health_check_fn(:5236) × 1（等待 first_node 就绪）
              ├── deploy::register_and_start_dmserver_service × (N-1)（other_nodes 串行）
              ├── health_check_fn(:5236+idx) × (N-1)
              └── deploy::verify_dsc_node × N（并行）
```

## 测试矩阵

| 测试名 | 覆盖 Gate / 场景 | 关键断言 | 结果 |
|--------|----------------|---------|------|
| test_run_with_runners_skips_completed_steps_from_checkpoint | 全部 8 个 gate（均为 true） | exec_log 不含 dm_service_installer.sh / dmasmcmd / dmasmtool / dminit control= | PASS |
| test_run_with_runners_calls_steps_in_order_no_checkpoint | 无 checkpoint，全流程 | dmasmcmd 在 dmasmtool 之前；all nodes 含 dmcss/dmasmsvr/dmserver；tar czf/xzf；SELECT STATUS$ | PASS |
| test_first_node_is_primary_role | first_node 角色判定 | Standby 排前时 dmasmcmd 仅在 Primary runner 日志中 | PASS |
| test_run_with_runners_returns_error_when_no_primary_node | 全 standby 无 Primary | 返回 Err，消息含 "primary" 或 "first_node" | PASS |
| test_checkpoint_saved_after_each_phase | Gate 5/6 中断 | dminit 失败后 css_asm_started=true / asm_diskgroup_created=true / dminit_shared_done=false | PASS |

## 与 RWS analog 的差异

| 维度 | RWS (rws/mod.rs) | DSC (dsc/mod.rs) |
|------|-----------------|-----------------|
| install 方式 | phases::run_install_phase（含 dminit） | run_dsc_install_all_nodes（不含 dminit，Pitfall 1） |
| gate 结构 | run_early_checkpoints + run_init_restore_checkpoints 双层 | 8 个 gate 直接展开在 run_with_runners |
| 专有 checkpoint 字段 | 无（用通用 5 字段） | 新增 6 个 DSC 专属字段 |
| first_node 概念 | Primary 节点（run_install_phase 内查找） | first_node_index() 在 Gate 5/6/7/8 中复用 |
| 健康检查端口 | dminit.port（5236） | dminit.port（dmserver）+ 9349（DMASM，Pitfall 2 防御） |
| config 分发 | distribute_configs（dmarch/dmmal/dmwatcher） | distribute_dsc_configs（dmdcr_cfg/dmasvrmal/dmdcr） + distribute_config_dir（tar 打包分发） |

## 人工验证报告

Task 2 是 `checkpoint:human-verify`，等待用户执行以下步骤：

1. `cargo build --release -p dm-database-installer` — **已验证**: release 构建通过，无 warning
2. `cargo run -p dm-database-installer -- install dsc --help` — 待用户执行
3. 准备最小 dsc.toml（oguid + [dsc_storage] + [[nodes]] primary/standby）— 待用户验证
4. 缺少 dsc_storage 时的错误提示验证 — 待用户执行
5. 无 SSH 可达时的错误消息验证（应含 SSH 连接失败，而非 "DSC 集群部署尚未实现"） — 待用户执行
6. 真实 DSC 测试环境（含 2 节点 + 4 块共享块设备）完整部署 — 需用户硬件环境
7. 中断后重跑验证 — 需用户硬件环境

## Task Commits

1. **Task 1 feat: 完整 DSC 入口编排** - `255d742`
   - 完整重写 `src/cluster/dsc/mod.rs`（889 行插入 / 5 行删除）
   - run() + run_with_runners() + 8 个 gate + 7 个私有 helper + 5 个单元测试
   - 全部 246 个测试通过，release 构建无 warning

## 已知未覆盖路径

1. **真实 dmasmcmd stdin pipe 行为**：测试只验证命令字符串格式，不验证 dmasmcmd 实际响应（需真实 DSC 环境 + 共享块设备验证）
2. **dmasmtool 磁盘组创建成功标志**：真实执行需 DMASM 服务已启动，共享存储 IO 正常
3. **distribute_config_dir tar 包内容正确性**：测试用 fake-tarball 字节，不验证真实 tar 包格式
4. **dmserver 跨节点时序**：other_nodes 的 dmserver 启动端口计算（`dminit.port + node_idx`）在真实环境中需验证端口正确配置

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] 修复测试并发竞争（set_current_dir racy）**
- **Found during:** Task 1 GREEN 阶段（运行多测试并发时 3/5 失败）
- **Issue:** 5 个测试均使用 `std::env::set_current_dir()` 切换 CWD（让 ClusterCheckpoint::load() 找到临时文件），并发运行时不同测试相互干扰
- **Fix:** 引入静态 `CWD_LOCK: Mutex<()>` 串行化所有测试，同时用 `unwrap_or_else(|e| e.into_inner())` 处理 Mutex 中毒问题
- **Files modified:** src/cluster/dsc/mod.rs（测试部分）
- **Verification:** 5 个测试全部通过

**2. [Rule 1 - Bug] 修复 dminit 命令前缀不匹配**
- **Found during:** Task 1 GREEN 阶段（Test 5 checkpoint 验证失败）
- **Issue:** MockRunner 的 pattern `"/opt/dmdbms/bin/dminit"` 不匹配实际命令前缀 `"'/opt/dmdbms'/bin/dminit"`（shell_quote 会给路径加单引号）
- **Fix:** 将 Test 5 的 pattern 改为 `"'/opt/dmdbms'/bin/dminit"` 与实际命令一致
- **Files modified:** src/cluster/dsc/mod.rs（测试部分）

**3. [Rule 2 - Missing critical] 添加 verify_dsc_node 响应预设**
- **Found during:** Task 1 GREEN 阶段（Test 2 verify 步骤失败）
- **Issue:** `verify_dsc_node` 调用 `echo 'SELECT STATUS$...'`，MockRunner 默认返回空 stdout，`output.to_uppercase().contains("OPEN")` 失败
- **Fix:** 在 Test 2 的 runner0/runner1 中预设 `echo 'SELECT STATUS$` 前缀响应，返回含 `OPEN\nNORMAL` 的 stdout
- **Files modified:** src/cluster/dsc/mod.rs（测试部分）

**Total deviations:** 3 auto-fixed（均为测试层面修正，无业务逻辑变更）

## Threat Surface Scan

无新增 threat surface：
- `src/cluster/dsc/mod.rs` 是编排层，不引入新的网络端点、认证路径或文件访问模式
- 所有 SSH 操作通过 Plan 03 的 deploy 函数执行，trust boundary 已在 07-03 threat model 中覆盖
- ClusterCheckpoint JSON 信任模型与 RWS/Dw 集群一致（T-07-12 accept）

## Self-Check: PASSED

- src/cluster/dsc/mod.rs 存在: FOUND
- 07-04-SUMMARY.md 存在: FOUND
- 提交 255d742 存在: FOUND
- bail!("DSC 集群部署尚未实现") 计数为 0: VERIFIED
- 5 个 DSC 测试全部通过: VERIFIED
- 全套 246 个测试通过: VERIFIED
- release 构建无 warning: VERIFIED
