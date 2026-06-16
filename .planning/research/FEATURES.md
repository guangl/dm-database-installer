# Feature Research

**Domain:** Database installer CLI tool (DM Database / 达梦数据库)
**Researched:** 2026-06-12
**Confidence:** HIGH (DM官方文档直接验证) / MEDIUM (类比工具推断)

## Feature Landscape

### Table Stakes (Users Expect These)

Features users assume exist. Missing these = product feels incomplete.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Silent/non-interactive install | 原生 DMInstall.bin 需要交互，自动化场景必须绕过 | LOW | 使用 `-q` 参数 + XML/ini 配置，DM 官方已支持静默模式 |
| dminit 参数配置 | 四个不可修改参数 (PAGE_SIZE/CHARSET/CASE_SENSITIVE/EXTENT_SIZE) 必须在初始化时正确设置 | LOW | 未正确设置等于强迫用户重建实例，是高频坑 |
| 端口配置 (PORT_NUM) | 同机多实例或端口冲突场景必须支持 | LOW | dminit 默认 5236，可配置 1024-65534 |
| 数据目录配置 | 生产环境不允许放在默认路径，DBA 必须自定义 | LOW | PATH/CTL_PATH/LOG_PATH 等多路径参数 |
| 实例名 / DB_NAME 配置 | 同机多实例区分靠名字 | LOW | INSTANCE_NAME 上限 16 字符，DB_NAME 上限 128 字符 |
| systemd 服务注册 | 生产机重启后必须自动拉起，否则故障 | LOW | 调用 `dm_service_installer.sh`，不同服务类型不同参数 |
| 预检查 (pre-flight checks) | 安装前发现问题比安装中失败成本低 100 倍 | MEDIUM | 参见下文检查项清单 |
| 进度/状态输出 | 静默执行没有反馈 = 用户不知道发生了什么，直接放弃 | LOW | 分步骤打印，每步有成功/失败标志 |
| 卸载 / 清理 | 安装失败或重装场景必需 | LOW | 停服务、注销服务、删安装目录；数据目录可选保留 |
| 操作日志文件 | DBA 必须能事后审计安装过程，生产变更要求 | LOW | 写到固定路径的 `.log` 文件，与 stdout 输出同步 |
| Linux x86/ARM 支持 | 达梦主力平台是国产信创 ARM（鲲鹏、飞腾），x86 是主流 | MEDIUM | 需要交叉编译两个 target；下载包 URL 路径不同 |
| Windows 支持 | 开发者本地环境 Windows 占比高，项目明确要求 | MEDIUM | Windows 服务注册机制不同于 systemd |
| 安装包自动下载 | 手动下载 .bin 对开发者极不友好，是 friction 最大的一步 | MEDIUM | 达梦官网提供下载，需检测平台自动选择 URL |
| 密码配置 (SYSDBA_PWD) | 安全要求不能用默认密码进生产 | LOW | 密码规则：9-48 字符，含大小写和数字；开发环境可使用内置默认值 |

**预检查 (pre-flight) 检查项清单（参考 TiUP/OBD/Oracle Installer）：**
- 可用内存 >= 1GB（DM 官方最低要求）
- 磁盘空间 >= 3GB（安装 1GB + 临时空间 2GB）
- 目标端口未被占用
- 目标路径可写
- 操作系统位数 (64-bit)
- dmdba 用户是否存在 / ulimit 设置
- /tmp 目录空间（DM 安装器需要 2GB 临时空间）
- 集群模式: SSH 连通性、目标节点时钟同步

---

### Differentiators (Competitive Advantage)

Features that set the product apart. Not required, but valued.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| `curl \| sh` 单行安装 | 开发者最低摩擦：一行命令拉起本地 DM 环境，类比 TiUP Playground | MEDIUM | 脚本包装 installer 二进制，自动下载、自动初始化、默认参数；需 HTTPS + checksum 验证 |
| TOML 配置文件驱动 | DBA 声明式部署：整个集群拓扑写进一个文件，可 Git 管理、可复审 | MEDIUM | 类比 TiUP topology YAML；支持 standalone/primary-standby/DSC/DPC 四种拓扑 |
| 单点 SSH 推送多节点 | DBA 只在控制机执行，不用 SSH 进每台节点手动操作，类比 TiUP cluster deploy | HIGH | 依赖 Rust openssh/russh；需要文件分发（安装包 SCP）+ 远程命令执行 |
| 主备集群一键部署 | 目前 DBA 需在 2 台机器上各自配置 dmarch.ini/dmmal.ini/dmwatcher.ini/dmmonitor.ini | HIGH | 参数联动性强（OGUID、MAL_PORT、归档路径需要主备对应）；错误极难排查 |
| DSC 集群一键部署 | 共享存储集群 (DMCSS+DMASMSVR+DMSERVER) 启动顺序严格，配置文件繁多 | HIGH | 需要 dmdcr_cfg.ini、ASM 磁盘组配置；启动顺序 DMCSS→DMASMSVR→DMSERVER |
| DPC 分布式集群一键部署 | MP/BP/SP 三类节点角色各自配置不同，手动部署是数日工作 | HIGH | MP 唯一、副本为奇数 >=3、SP 无副本只能横向扩展；dmarch.ini RAFT_VOTE_INTERVAL 节点间不同 |
| --dry-run 模式 | 显示将执行的步骤但不实际操作，DBA 在生产变更前必须先审查 | LOW | 打印所有拟执行命令和文件变更，无副作用 |
| 幂等重试 | 安装中途失败后重跑不会报"已存在"错误，继续未完成的步骤 | MEDIUM | 需要状态跟踪（步骤完成标记文件）；避免重复 dminit 覆盖已有实例 |
| 安装结果校验 | 安装完成后自动连接数据库验证可用性，而非"安装完成"就结束 | LOW | 执行简单 SQL 检查（SELECT 1 或查询 V$VERSION） |
| 配置文件校验（validate 子命令） | TOML 配置写错不到运行时不知道，提前校验节省 DBA 时间 | LOW | 独立运行不执行安装，检查拓扑一致性、必填项、值范围 |

---

### Anti-Features (Commonly Requested, Often Problematic)

Features that seem good but create problems.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| 交互式向导 (interactive wizard) | 对不了解参数的新用户友好 | 无法脚本化；`curl \| sh` 场景不兼容；增加代码路径复杂度 | 提供带注释的示例 TOML 文件；`--dry-run` 预览 |
| 多版本管理 (version matrix) | 用户可能想固定旧版本 | 达梦官网只提供最新版，无多版本下载；版本兼容矩阵维护成本高 | 固定最新版；高级需求由用户自行管理安装包 |
| 升级/迁移 (upgrade/migrate) | 现有数据库需要升级 | 涉及备份验证、数据迁移、回滚，复杂度是全新安装的 5 倍以上；范围蔓延风险极高 | 明确 Out of Scope；指引用户使用达梦官方 DMDTS 工具 |
| 容器/K8s 部署 | 现代基础设施首选 | 达梦官方 Docker 镜像授权复杂；K8s Operator 需要独立设计；与 SSH 部署模型冲突 | 明确 Out of Scope for v1；后续独立 Operator 项目 |
| 图形界面 (GUI/TUI) | 直观，降低学习曲线 | 与 CLI 工具目标冲突；`curl \| sh` 场景不支持 TUI | 清晰的 `--help` 输出；提供 `--dry-run` 和 `validate` 替代 |
| 实时监控仪表盘 | 类比 TiUP 内置 Grafana | 超出安装器职责范围；运维监控是独立系统 | 输出连接信息，让用户接入现有监控系统 |
| 自动备份计划配置 | 顺手做了省事 | 备份策略高度依赖业务需求，错误配置比不配置更危险 | 安装后输出备份配置建议文档链接 |

---

## Feature Dependencies

```
[curl|sh 单行安装]
    └──requires──> [安装包自动下载]
                       └──requires──> [平台检测 (arch/OS)]
    └──requires──> [dminit 参数配置]
    └──requires──> [systemd 服务注册]
    └──requires──> [进度/状态输出]

[主备集群部署]
    └──requires──> [单机安装] (先在每个节点装好软件)
    └──requires──> [SSH 推送多节点]
    └──requires──> [dmmal.ini 配置生成]
    └──requires──> [dmarch.ini 配置生成]
    └──requires──> [dmwatcher.ini 配置生成]
    └──requires──> [dmmonitor.ini 配置生成]

[DSC 集群部署]
    └──requires──> [单机安装]
    └──requires──> [SSH 推送多节点]
    └──requires──> [dmdcr_cfg.ini 配置生成]
    └──requires──> [ASM 磁盘组预配置] (外部依赖，需用户预先准备)

[DPC 集群部署]
    └──requires──> [单机安装]
    └──requires──> [SSH 推送多节点]
    └──requires──> [MP/BP/SP 角色分配与配置生成]
    └──requires──> [dmarch.ini RAFT_VOTE_INTERVAL 差异化配置]

[TOML 配置文件驱动]
    └──enhances──> [主备/DSC/DPC 集群部署] (所有集群拓扑的声明式入口)

[--dry-run 模式]
    └──enhances──> [TOML 配置文件驱动] (先预览再执行)

[配置文件校验 (validate)]
    └──requires──> [TOML 配置文件驱动]
    └──enhances──> [预检查 (pre-flight checks)]

[幂等重试]
    └──requires──> [进度/状态输出] (状态跟踪依赖步骤标记)

[安装包自动下载] ──conflicts──> [Windows 静默安装]
    (Windows .exe 安装包与 Linux .bin 流程不同，需要分支处理)
```

### Dependency Notes

- **主备集群 requires 单机安装:** 每个节点都需要先装好达梦数据库软件，才能配置集群模式；单机安装是所有集群模式的基础
- **DSC requires 外部 ASM 磁盘:** 共享存储 (裸设备或 ASM 磁盘组) 必须由用户/基础设施预先准备，installer 无法自动创建共享存储
- **DPC requires 奇数节点 >=3:** MP 和 BP 副本数为奇数，最少 3 节点；这是 RAFT 共识协议要求，配置校验必须强制检查
- **幂等重试 requires 步骤状态跟踪:** 需要在目标目录写入已完成步骤的标记，防止重跑时重复执行 dminit（dminit 不是幂等的，重跑会报错）

---

## MVP Definition

### Launch With (v1)

Minimum viable product — what's needed to validate the concept.

- [ ] `curl | sh` 单行安装单机达梦 — 核心开发者价值主张，没有这个就没有差异化
- [ ] TOML 配置文件驱动的单机安装 — DBA 受众的最小可用功能
- [ ] dminit 关键参数配置 (PAGE_SIZE/CHARSET/PORT/PATH/PASSWORD) — 缺少即无法产出可用实例
- [ ] systemd 服务注册 (Linux) — 没有这个生产机重启后数据库不起，DBA 不会接受
- [ ] 基础预检查 (内存/磁盘/端口) — 降低安装失败率，提升用户信心
- [ ] 操作日志文件 — 生产变更必须有日志，否则 DBA 无法使用
- [ ] 安装包自动下载 (Linux x86/ARM) — `curl|sh` 流程的核心前提

### Add After Validation (v1.x)

Features to add once core is working.

- [ ] 主备集群部署 — 验证 SSH 推送框架后再构建集群模式
- [ ] --dry-run 模式 — 用户反馈需要预览功能时加入
- [ ] 配置文件 validate 子命令 — 主备/集群配置复杂度上来后必要
- [ ] Windows 支持 — 补充开发者平台覆盖
- [ ] 幂等重试 — 集群部署失败重试场景驱动

### Future Consideration (v2+)

Features to defer until product-market fit is established.

- [ ] DSC 共享存储集群部署 — 依赖共享存储基础设施，用户群体更小，复杂度最高
- [ ] DPC 分布式集群部署 — 架构最复杂（MP/BP/SP 三角色），适合 v2
- [ ] 安装结果自动校验 (连接验证) — 锦上添花，v1 用户自己能验证
- [ ] 容器/K8s 部署 — Out of Scope for v1，独立立项

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| `curl\|sh` 单行安装 | HIGH | MEDIUM | P1 |
| dminit 参数配置 | HIGH | LOW | P1 |
| 安装包自动下载 | HIGH | MEDIUM | P1 |
| 预检查 (pre-flight) | HIGH | MEDIUM | P1 |
| systemd 服务注册 | HIGH | LOW | P1 |
| 操作日志 | HIGH | LOW | P1 |
| TOML 配置驱动单机 | HIGH | LOW | P1 |
| 进度/状态输出 | MEDIUM | LOW | P1 |
| 主备集群部署 | HIGH | HIGH | P2 |
| --dry-run 模式 | MEDIUM | LOW | P2 |
| 配置 validate 子命令 | MEDIUM | LOW | P2 |
| Windows 支持 | MEDIUM | MEDIUM | P2 |
| 幂等重试 | MEDIUM | MEDIUM | P2 |
| 安装结果校验 | LOW | LOW | P2 |
| DSC 集群部署 | MEDIUM | HIGH | P3 |
| DPC 集群部署 | MEDIUM | HIGH | P3 |
| 卸载/清理 | MEDIUM | LOW | P2 |

**Priority key:**
- P1: Must have for launch
- P2: Should have, add when possible
- P3: Nice to have, future consideration

---

## Competitor Feature Analysis

| Feature | TiUP (TiDB) | OBD (OceanBase) | DM Installer (ours) |
|---------|-------------|-----------------|---------------------|
| 单行快速安装 | `tiup playground` 一行命令 | `obd demo` 一行命令 | `curl\|sh` 触发 installer |
| 配置文件格式 | YAML topology 文件 | YAML 配置文件 | TOML (Rust 生态首选) |
| 集群部署 | `tiup cluster deploy` SSH 远程 | `obd cluster deploy` | SSH 远程推送，单点控制 |
| 预检查 | `tiup cluster check --apply` 自动修复 | 内置环境检测 | pre-flight，无自动修复 (v1) |
| 多集群管理 | `tiup cluster list` 管理多集群 | 支持多集群 | v1 不需要 (单一部署工具) |
| Dry-run | 有 (--check only) | 部分支持 | --dry-run 子命令 |
| 回滚 | 有 (upgrade 失败回滚) | 部分支持 | v1 仅 cleanup，不支持事务回滚 |
| 监控组件 | 自动部署 Grafana/Prometheus | 自动部署 OBAgent | Out of Scope |
| GUI | 无 | `obd web` GUI 模式 | Out of Scope |
| 安装包管理 | 自建 mirror 服务 | 本地 repo 缓存 | 直接下载官网包 |

---

## DM-Specific Technical Requirements

达梦数据库安装的特殊性，直接影响 feature 实现方式：

### 不可修改的初始化参数（高风险）

下列参数一旦 dminit 执行后 **无法修改**，必须在安装前明确配置：

| 参数 | 含义 | 默认值 | 生产建议 |
|------|------|--------|---------|
| PAGE_SIZE | 数据页大小 (KB) | 8 | OLTP 用 8，OLAP 用 16/32 |
| EXTENT_SIZE | 簇大小 (页数) | 16 | 通常保持默认 |
| CASE_SENSITIVE | 大小写敏感 | Y | 建议明确设置，与应用对齐 |
| CHARSET | 字符集 (0=GB18030, 1=UTF-8) | 0 | 新项目用 UTF-8 (1) |

**implication:** installer 必须在 TOML 中强制这四个参数并加注释警告，不能用隐含默认值。

### 集群模式配置文件矩阵

| 集群类型 | 需要的配置文件 |
|---------|--------------|
| 单机 | dm.ini, dminit 参数 |
| 主备 | dm.ini + dmarch.ini + dmmal.ini + dmwatcher.ini + dmmonitor.ini |
| DSC | dm.ini + dmdcr_cfg.ini + ASM 磁盘配置 |
| DPC | dm.ini + dmarch.ini (RAFT 差异化) + 节点角色配置 |

### 服务注册命令

```bash
# Linux systemd
/dm/dmdbms/script/root/dm_service_installer.sh -t dmserver -p <instance_name> -dm_ini /dm/data/<INST>/dm.ini
systemctl enable DmService<instance_name>.service
systemctl start  DmService<instance_name>.service

# 主备守护进程
/dm/dmdbms/script/root/dm_service_installer.sh -t dmwatcher -watcher_ini /dm/data/dmwatcher.ini
```

---

## Sources

- [达梦 dminit 参数详解 | 达梦技术文档](https://eco.dameng.com/document/dm/zh-cn/pm/dminit-parameters.html) — HIGH confidence
- [达梦安装及卸载 | 达梦技术文档](https://eco.dameng.com/document/dm/zh-cn/pm/install-uninstall.html) — HIGH confidence
- [TiUP Cluster 部署文档 | PingCAP](https://docs.pingcap.com/tidb/stable/tiup-cluster/) — HIGH confidence
- [TiDB 生产部署 | PingCAP](https://docs.pingcap.com/tidb/stable/production-deployment-using-tiup/) — HIGH confidence
- [OceanBase OBD 文档](https://en.oceanbase.com/docs/obd-en) — MEDIUM confidence
- [DPC 分布式集群安装部署 | 达梦技术文档](https://eco.dameng.com/document/dm/zh-cn/ops/DPC_installation_cluster.html) — HIGH confidence
- [达梦主备集群安装指南 | 达梦技术社区](https://eco.dameng.com/community/article/27ce45026f59e14410ecaf1f82298127) — MEDIUM confidence (社区文章)
- [DSC 共享存储集群 | 达梦技术文档](https://eco.dameng.com/document/dm/zh-cn/start/dm-asm-cluster.html) — HIGH confidence
- [达梦服务注册 | 达梦技术社区](https://eco.dameng.com/community/training/151fd9125a20e4b4b3a8553d9578a96e) — MEDIUM confidence

---
*Feature research for: DM database installer CLI (达梦数据库安装器)*
*Researched: 2026-06-12*
