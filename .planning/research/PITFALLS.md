# Pitfalls Research

**Domain:** 达梦数据库安装器 CLI (Database Installer Automation Tool)
**Researched:** 2026-06-12
**Confidence:** HIGH (DM-specific pitfalls verified against official docs; SSH/Rust patterns verified against community sources)

---

## Critical Pitfalls

### Pitfall 1: dminit 不可变参数选错导致必须重装

**What goes wrong:**
`dminit` 初始化参数中有若干参数一旦确定无法修改，包括字符集（`CHARSET`）、大小写敏感（`CASE_SENSITIVE`）、页大小（`PAGE_SIZE`）、簇大小（`EXTENT_SIZE`）、空格填充模式（`BLANK_PAD_MODE`）。用户在安装阶段选错任意一个，唯一的补救方案是重新初始化实例并导入数据——相当于重装。

**Why it happens:**
安装工具把这些参数当作普通的"可配置项"展示，没有明确标注"一旦设定永久生效"。用户在测试时随手选了默认值（如 GB18030），到生产迁移 MySQL 数据时才发现需要 UTF-8。

**How to avoid:**
- TOML 配置中对这些参数加注释：`# WARNING: 创建后不可更改`
- 安装前打印摘要并要求用户确认这些不可变参数
- 提供明确的默认建议值：新项目推荐 UTF-8（charset=1），从 Oracle 迁移推荐大小写敏感，从 MySQL 迁移推荐大小写不敏感
- 在 `--dry-run` 模式下显著展示这些不可变参数

**Warning signs:**
- 用户在安装完成后询问"如何改字符集"
- 测试环境和生产环境字符集不一致

**Phase to address:** 单机安装 Phase（TOML 配置解析 + 安装前确认阶段）

---

### Pitfall 2: 禁止 root 用户安装，但 root 执行 post-install 脚本是必须的

**What goes wrong:**
达梦官方要求必须用 `dmdba` 用户安装，禁止 root 安装。但安装后需要以 root 执行 `root_installer.sh` 才能完成 systemd 服务注册等系统级操作。如果安装工具以 root 运行，安装完成后数据库目录归 root 所有，`dmdba` 用户无法访问，数据库无法启动；如果以 dmdba 运行但没有自动执行 root_installer.sh，安装看似完成但服务注册缺失。

**Why it happens:**
双阶段权限模型（非 root 安装 + root post-install）对于自动化工具不直观。curl|sh 场景下用户往往直接以 root 运行。

**How to avoid:**
- 检测当前用户是否为 root，若是则拒绝并提示创建 dmdba 用户
- 或者：若以 root 运行，则自动创建 dmdba 用户，切换后以 dmdba 执行安装，再切回 root 执行 `root_installer.sh`（实现 setuid/sudo 切换流程）
- 安装器自身需要知道"是否有 sudo 权限"来决定能否自动完成 post-install 步骤

**Warning signs:**
- 安装后 `systemctl status DmServiceDMSERVER` 找不到服务
- dmdba 用户无法读写 DM_HOME 目录

**Phase to address:** 单机安装 Phase（用户创建、权限处理逻辑）

---

### Pitfall 3: SSH sudo 无 TTY 导致远程集群安装中断

**What goes wrong:**
通过 SSH 执行远程命令时，若目标节点 sudo 配置要求 TTY（`requiretty`），远程执行 `sudo command` 会报错 `sudo: no tty present and no askpass program specified` 或 `sudo: sorry, you must have a tty to run sudo`，整个集群安装流程中断。

**Why it happens:**
RHEL/CentOS 默认 sudoers 包含 `Defaults requiretty`，SSH exec channel 不分配 pseudo-TTY。自动化工具默认不请求 PTY。

**How to avoid:**
- 在 SSH 连接时分配 PTY（`-t` 选项），但这会引入交互模式副作用
- 更好的方法：要求用户在 sudoers 中为 dmdba 用户配置 NOPASSWD，并在预检阶段验证
- 安装器的预检步骤应验证：`ssh user@host "sudo -n true"` 是否成功，失败则提前报错并给出修复指引
- 支持在 TOML 中配置 `become_method = "sudo"` 或 `become_method = "su"`

**Warning signs:**
- 预检 SSH 连通性通过，但安装步骤在 sudo 命令时失败
- 日志中出现 `requiretty` 关键字

**Phase to address:** 主备/集群安装 Phase（SSH 预检 + 远程命令执行层）

---

### Pitfall 4: 达梦官网下载需要登录认证，无公开直链

**What goes wrong:**
达梦官网（dameng.com）的安装包下载需要用户注册并登录，没有像 PostgreSQL/MySQL 那样的无认证直接下载链接。自动下载功能如果依赖直接 URL 会失败（302 重定向到登录页或返回 HTML）。

**Why it happens:**
国产数据库的商业模式要求用户注册才能下载（用于收集用户信息、授权管理）。试用版虽然免费，但不提供匿名直链。

**How to avoid:**
- 不要假设可以无认证爬取官网下载链接
- 设计两种模式：(a) 用户预先下载安装包，安装器接受本地路径；(b) 提供官方下载页面 URL，指导用户手动下载
- 可支持从配置中读取已登录的 Cookie/Token，但这是边缘功能
- 如果达梦生态社区（eco.dameng.com）提供 API，优先使用

**Warning signs:**
- 自动下载返回 HTML 页面而非二进制文件
- 下载链接 checksum 不匹配（实际下载的是登录页 HTML）

**Phase to address:** 下载/分发 Phase（auto-download 功能设计阶段需要提前验证）

---

### Pitfall 5: cluster 节点中途失败导致 DCR 磁盘写入脏数据

**What goes wrong:**
DSC/DPC 集群安装过程中，如果某个节点在配置阶段崩溃或网络中断，DCR（集群注册）磁盘中可能记录了部分节点信息或故障标记。再次启动时，DMCSS 检测到节点标记为 error 状态，整个集群无法启动。

**Why it happens:**
DMDSC 的 DCR 磁盘是集群的状态存储，节点故障会被自动标记。安装工具中断后无法自动清理这个标记；手动清理需要 `dmasmcmd clear dcrdisk err_ep_arr`，大多数用户不知道这个命令。

**How to avoid:**
- 集群安装必须实现原子化步骤 + 清理逻辑：每个步骤要么完成要么回滚，不能留下中间状态
- 安装失败时提供明确的"清理命令"输出，告知用户如何清理 DCR
- 实现 `dm-installer cluster clean` 子命令，自动检测并清理 DCR 脏状态
- 安装器在每个关键步骤前保存状态文件（`.dm-installer-state.json`），支持断点续装

**Warning signs:**
- 集群安装中途 Ctrl+C 或网络断开
- 重新运行安装时提示"节点已存在"或集群无法 OPEN

**Phase to address:** 集群安装 Phase（状态管理 + 失败恢复设计）

---

### Pitfall 6: curl|sh 脚本被网络中断截断执行

**What goes wrong:**
`curl https://example.com/install.sh | sh` 模式下，如果网络在传输中途断开，curl 退出但 sh 已经在执行部分脚本内容。截断的 shell 脚本可能在执行一半时停止，留下已创建的目录、用户、部分解压的文件，但没有完成安装。更危险的是：bash 对部分读取的函数定义行为未定义。

**Why it happens:**
`curl` 和 `sh` 是两个独立进程，管道不保证原子性。

**How to avoid:**
- 安装 shell 脚本将所有逻辑包裹在一个主函数中，最后一行才调用 `main "$@"` 。这样即使脚本被截断，函数体还未执行
- 脚本开头做完整性检查（如末尾有特定标记行）
- 推荐用户使用 `curl -fsSL https://... -o install.sh && sh install.sh` 而非直接管道
- 文档中明确说明两种方式，并注明管道方式的风险

**Warning signs:**
- 安装后发现部分文件存在但服务未启动
- 用户报告"安装卡住了"

**Phase to address:** curl|sh 分发 Phase（安装脚本设计）

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| 硬编码达梦安装包 URL 路径规律 | 快速实现自动下载 | 官网改版后全面失效，无法感知 | 绝不接受，应优先设计为用户指定路径 |
| 跳过 SSH 预检直接安装 | 减少代码量 | 集群安装在不兼容环境下进行到一半才失败，状态难清理 | 绝不接受 |
| 直接 panic 代替错误处理 | 开发速度快 | 用户看到 Rust backtrace，体验极差；生产环境无法诊断 | 仅 prototype 阶段 |
| 不保存安装状态，每次从头开始 | 实现简单 | 集群中断后用户必须手动清理再重装 | MVP 单机模式可接受，集群模式绝不接受 |
| 用 `#[serde(deny_unknown_fields)]` 而不加 `version` 字段 | 配置解析严格 | 升级安装器后旧 TOML 配置全部报错 | 绝不接受 |
| 不验证下载文件 checksum | 减少网络请求 | 静默安装损坏的文件，故障难排查 | 绝不接受 |

---

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| 达梦 `dminit` 命令 | 以 root 执行 dminit，导致数据目录归属 root | 必须 su/sudo 切换到 dmdba 用户后执行 |
| 达梦 `dm_service_installer.sh` | 忘记以 root 执行 post-install 脚本 | 安装器主流程结束后，显式提示或自动执行 root 阶段脚本 |
| 达梦 `dm.key` license 文件 | 没有 key 文件仍可安装，但集群模式启动失败 | 集群模式安装前验证 key 文件包含对应集群类型授权（CLUSTER_TYPE 第4位=1 for DSC） |
| SSH `sudo` 远程执行 | 假设目标机已配置 NOPASSWD | 预检阶段验证 `sudo -n true`，失败则给出 sudoers 配置模板 |
| 达梦官网下载 | 假设下载 URL 稳定 | 提供用户自定义安装包路径，自动下载作为可选功能 |
| TOML 配置文件 | `deny_unknown_fields` 导致向前兼容失败 | 配置结构体使用 `#[serde(default)]`，不使用 `deny_unknown_fields` |
| Linux `/etc/security/limits.conf` | 修改后当前 SSH 会话不生效 | 安装器执行时用 `ulimit` 临时设置，并在最后提醒重新登录或 reboot |

---

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| 串行逐节点 SSH 操作集群 | 3 节点 DSC 安装耗时是单机的 3 倍以上 | 识别可并行的步骤（文件分发、参数配置），用 tokio::join! 并发执行 | 超过 3 节点时耗时线性增长不可接受 |
| 每次安装前重新下载安装包 | 反复安装时等待下载 | 检测本地缓存，checksum 匹配则跳过下载 | 网络不稳定环境 |
| SFTP 逐文件传输大安装包 | 安装包 600MB+，SFTP 传输极慢 | 先本地校验，支持 rsync/scp 作为备用传输方式 | 安装包需要分发到多节点时 |

---

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| 在 TOML 配置文件中明文存储 SSH 密码 | 配置文件泄露即等同于服务器密码泄露 | 优先使用 SSH 密钥认证；密码使用环境变量注入，不持久化到文件 |
| curl\|sh 未强制 HTTPS | MITM 攻击注入恶意命令 | 安装脚本 URL 必须 HTTPS，脚本本身检查 `$HTTPS_*` 环境变量 |
| 以 root 运行整个安装流程 | 安装脚本 bug 可直接破坏系统 | 仅在必须时（post-install systemd 注册）提权，其余步骤以 dmdba 用户运行 |
| 不验证下载文件来源 | 供应链攻击 | 提供官方 checksum（MD5/SHA256），下载后强制校验，校验失败拒绝安装 |
| TOML 中的连接用户名/密码记录在日志中 | 日志泄露敏感信息 | 日志中 mask 密码字段，`password = "****"` |

---

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| 安装失败只输出 Rust panic backtrace | 普通用户无法理解错误，无法自行修复 | 捕获所有错误，翻译为人类可读的中文诊断信息，附上具体修复步骤 |
| 不可变参数（字符集/大小写）默认静默设置 | 用户安装完成后才发现参数不对，需要重装 | 安装前明确展示不可变参数摘要，要求确认或 `--yes` 跳过 |
| 集群安装无进度反馈 | 多节点安装耗时长，用户不知道是否卡住 | 每个步骤输出带时间戳的状态行，如 `[2/7] node1: 正在传输安装包 (412MB/600MB)...` |
| 安装成功但没有验证步骤 | 用户不确定是否真的成功 | 安装完成后自动执行连通性测试（isql 连接测试），输出 `SYSTEM IS READY` 确认 |
| 对已安装环境静默覆盖 | 用户二次运行时无意删除数据 | 检测到已安装时询问用户是否 (a) 跳过 (b) 重新初始化 (c) 退出 |

---

## "Looks Done But Isn't" Checklist

- [ ] **字符集/不可变参数:** 安装流程中是否明确展示并要求确认 CHARSET、CASE_SENSITIVE、PAGE_SIZE？
- [ ] **post-install root 脚本:** 安装完成后是否执行了 `root_installer.sh` 完成 systemd 服务注册？
- [ ] **dm.key 集群授权验证:** 集群模式安装前是否验证了 dm.key 中 CLUSTER_TYPE 对应位为 1？
- [ ] **幂等性:** 对已安装实例再次运行安装器是否有合理的检测和提示，而非静默覆盖？
- [ ] **SSH sudo 预检:** 集群模式是否在安装前验证所有节点的 SSH 连通性 + sudo NOPASSWD 配置？
- [ ] **DCR 清理命令:** 集群安装失败时是否输出清理 DCR 脏数据的具体命令？
- [ ] **下载校验:** 自动下载（若实现）是否验证 checksum 并在校验失败时拒绝继续？
- [ ] **curl|sh 截断防护:** 安装脚本是否将逻辑包裹在函数中，末尾才调用 main？
- [ ] **文件描述符限制:** 安装前是否检查并临时设置 `ulimit -n 65536`？
- [ ] **overcommit_memory:** 是否检查 `/proc/sys/vm/overcommit_memory` 值（应为 0 或 1，非 2）？

---

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| 字符集选错 | HIGH | 备份数据 → `rm -rf` 数据目录 → 重新 `dminit` → 导入数据 |
| DSC DCR 脏数据 | MEDIUM | `dmasmcmd clear dcrdisk err_ep_arr` → 重启 DMCSS → 重启各节点 DMSERVER |
| post-install 脚本未执行 | LOW | 手动以 root 执行 `/home/dmdba/script/root/root_installer.sh` |
| SSH sudo 失败 | LOW | 在目标节点 sudoers 中为 dmdba 添加 NOPASSWD 条目，重新运行集群安装 |
| 安装包 checksum 不匹配 | LOW | 删除缓存文件，重新下载；若持续失败，切换到本地安装包模式 |
| 部分安装状态残留 | MEDIUM | 运行 `dm-installer cleanup` 子命令，或按安装器输出的清理指令手动操作 |
| curl\|sh 截断后状态混乱 | MEDIUM | 删除 DM_HOME 目录、dmdba 用户、systemd 服务文件，重新执行安装 |

---

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| dminit 不可变参数选错 | 单机安装 Phase（配置解析 + 安装确认） | 安装完成后 `SELECT UNICODE()` 和 `SELECT SF_GET_CASE_SENSITIVE_FLAG()` 验证 |
| root 安装 / post-install 脚本缺失 | 单机安装 Phase（权限处理逻辑） | 验证 `systemctl status DmServiceDMSERVER` 正常 |
| SSH sudo 无 TTY | 主备/集群安装 Phase（SSH 预检层） | 预检 `ssh user@host "sudo -n true"` 返回 0 |
| 达梦官网无公开直链 | 下载/分发设计 Phase（架构决策） | 用户提供本地路径时安装成功，不依赖网络下载 |
| 集群中途失败 DCR 脏数据 | 集群安装 Phase（状态管理设计） | 模拟节点故障后重新安装可以成功清理并重装 |
| curl\|sh 截断 | curl\|sh 分发 Phase（安装脚本设计） | 用 `head -100 install.sh | bash` 模拟截断，验证无副作用 |
| dm.key 集群授权缺失 | 集群安装 Phase（安装前验证） | 验证 key 文件解析逻辑可正确判断 CLUSTER_TYPE |
| 配置 schema 破坏向前兼容 | 配置系统 Phase（TOML 解析层设计） | 旧版 TOML 在新版安装器上 `--dry-run` 无报错 |
| 不可变参数默认静默设置 | 单机安装 Phase | `--dry-run` 输出包含所有不可变参数值 |
| 二次运行覆盖数据 | 任意安装 Phase（幂等性检测） | 对已安装实例运行安装器，提示"已安装"而非静默覆盖 |

---

## Sources

- [达梦安装前准备 - 官方文档](https://eco.dameng.com/document/dm/zh-cn/start/install-dm-linux-prepare.html)
- [达梦单机安装常见问题 - 官方 FAQ](https://eco.dameng.com/document/dm/zh-cn/faq/faq-dm-install.html)
- [达梦集群安装部署问题 - 官方 FAQ](https://eco.dameng.com/document/dm/zh-cn/faq/faq-dm-cluster.html)
- [DMDSC 注意事项 - 官方文档](https://eco.dameng.com/document/dm/zh-cn/pm/dsc-instructions-use.html)
- [dminit 参数详解 - 官方文档](https://eco.dameng.com/document/dm/zh-cn/pm/dminit-parameters.html)
- [curl bash pipe 安全讨论 - Kicksecure](https://www.kicksecure.com/wiki/Dev/curl_bash_pipe)
- [The hidden dangers of piping curl - Darian Moody](https://www.djm.org.uk/posts/protect-yourself-from-non-obvious-dangers-curl-url-pipe-sh/)
- [sudo no tty present 解决方法](https://www.simplified.guide/ssh/sudo-no-tty-askpass)
- [Rust 跨平台路径处理 - Sling Academy](https://www.slingacademy.com/article/creating-cross-platform-paths-and-file-operations-in-rust/)
- [达梦字符集不可修改 - CSDN](https://blog.csdn.net/ss7817258/article/details/119118598)
- [达梦 dm.key 授权说明 - cnDBA](https://www.cndba.cn/dave/article/4298)

---

*Pitfalls research for: 达梦数据库安装器 CLI (dm-database-installer)*
*Researched: 2026-06-12*
