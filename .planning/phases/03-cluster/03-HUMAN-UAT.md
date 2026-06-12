---
status: partial
phase: 03-cluster
source: [03-VERIFICATION.md]
started: 2026-06-12T23:48:05Z
updated: 2026-06-12T23:48:05Z
---

## Current Test

[awaiting human testing]

## Tests

### 1. 端到端双节点集群部署

expected: 在两台真实 Linux 节点（或 Docker sshd 容器）上运行 `dm-installer cluster deploy --config cluster.toml`，确认：
- 运行时日志中"主节点就绪"行早于"启动达梦备实例"行（CLUS-02 SC3）
- 两台节点实际完成 DM 安装并建立主备复制关系（CLUS-01 SC1）
result: [pending]

## Summary

total: 1
passed: 0
issues: 0
pending: 1
skipped: 0
blocked: 0

## Gaps
