---
slug: standalone-checkpoint-resume
date: 2026-06-14
status: complete
---

# standalone 安装中断续传 — SUMMARY

## 完成内容

- 新增 `src/standalone/checkpoint.rs`：Checkpoint 结构体，JSON 序列化，存 CWD/dm_installer_checkpoint.json
- `standalone/mod.rs`：run() 加载检查点恢复密码 + 跳过已完成步骤（silent_install / dminit）
- 删除 `idempotent.rs`（检测路径有误，被新的路径检查替代）
- 修复测试类型歧义（`serde_json` 引入 `PartialEq<Value> for u8`）
- Cargo.toml 增加 `serde_json = "1"`

## 跳过逻辑

| 步骤 | 跳过条件 |
|------|---------|
| silent_install | `cp.installed == true` OR `{install_path}/bin/dminit` 存在 |
| dminit | `{data_path}/dm.ini` 存在 |

## 测试

129 tests passed（新增 5 个 checkpoint 单测）
