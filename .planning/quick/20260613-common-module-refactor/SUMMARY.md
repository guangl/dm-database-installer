---
slug: common-module-refactor
status: complete
completed: 2026-06-13
commit: 8c64870
---

# Summary

重构完成。121 tests 全部通过，`cargo build` 无错误。

## 变更

- `src/common/ssh.rs` — 从 `cluster/ssh.rs` 移入（rename 99%）
- `src/common/sysinfo.rs` — 从 `download/detect.rs` 提取
- `src/common/download/` — `download/` 整体移入，`detect` 引用改为 `sysinfo`，`include_str` 路径更新
- `src/config/ssh.rs` — `SshCredentials` 从 `config/cluster.rs` 分离，`cluster.rs` 通过 `pub use` 保持兼容
- 6 个调用处引用路径全部更新（`standalone/mod.rs`、`cluster/deploy.rs`、`preflight.rs`、`primary_standby/mod.rs`）
