# Phase 6: status 命令 — Discussion Log

**Date:** 2026-06-14
**Mode:** --auto (fully autonomous, no user prompts)

## Areas Discussed

### 1. No-config 行为
- **Q:** config.toml 不存在时 status 如何响应？
- **Selected:** 仅显示本地状态，不报错（recommended default）
- **Rationale:** 与 guide.rs 风格一致，降低使用摩擦

### 2. 角色查询方式
- **Q:** 如何获取数据库角色（PRIMARY/STANDBY）?
- **Selected:** 端口监听 + V$INSTANCE SQL（disql SYSDBA/SYSDBA）（recommended default）
- **Rationale:** 与 phases.rs 现有模式一致，信息更丰富

### 3. 凭据来源
- **Q:** disql 连接用什么密码?
- **Selected:** 延用当前 SYSDBA/SYSDBA 硬编码模式（recommended default）
- **Rationale:** 在范围内；CR-02 是已知问题，留后续 phase 修复

### 4. 输出格式
- **Q:** 表格列和错误行如何呈现?
- **Selected:** 手动对齐表格，列：Node | Host | Process | Port | Role（recommended default）
- **Rationale:** 零额外依赖，简洁

### 5. 并发查询
- **Q:** 多节点是串行还是并行查询?
- **Selected:** tokio::join_all 并行（recommended default）
- **Rationale:** UX 更好，与集群部署并发模式一致

### 6. 本地节点检测
- **Q:** 如何检测本地 DM 进程?
- **Selected:** std::process::Command（ps aux | grep dmserver | grep -v grep）+ TCP 端口检测（recommended default）
- **Rationale:** 轻量，无 SSH，与 preflight.rs 端口检测模式一致

## Deferred Ideas

- `--watch` 模式 — future phase
- JSON 输出格式（--format json）— future phase
- SSH 端口可配置（CR-01 修复）— future phase
- SYSDBA 密码可配置（CR-02 修复）— future phase
- Windows 本地进程检测 — future platform work
