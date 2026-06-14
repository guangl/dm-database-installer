# Phase 2: TOML 配置驱动单机 - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-06-12
**Phase:** 2-TOML 配置驱动单机
**Mode:** --auto (all decisions auto-selected, no interactive prompts)
**Areas discussed:** 配置字段语义验证规则, --config 与参数确认UI联动, validate/install验证逻辑复用

---

## 配置字段语义验证规则

| Option | Description | Selected |
|--------|-------------|----------|
| Rust 层语义验证 | 在 `validate_install_config()` 检查 page_size/charset/extent_size 有效值集合，给出字段级错误消息 | ✓ |
| 依赖 dminit 错误 | 只做 TOML 类型检查，让 dminit 返回非零时捕获错误 | |

**Auto-selected:** Rust 层语义验证（recommended default）
**Notes:** INST-02 SC3 明确要求错误信息指向具体字段；dminit 的错误信息不够友好。

---

## --config 与参数确认 UI 联动

| Option | Description | Selected |
|--------|-------------|----------|
| 仍然展示确认 | 提供 --config 时展示配置文件里的实际值，等待 y/n 确认；--yes/--defaults 跳过 | ✓ |
| --config 自动跳过确认 | 提供配置文件 = 用户已知道参数，自动跳过不可修改参数确认提示 | |

**Auto-selected:** 仍然展示确认（recommended default）
**Notes:** 不可修改参数是安全网；与 Phase 1 行为一致；用户若想无交互可加 --yes。

---

## validate 与 install 验证逻辑复用

| Option | Description | Selected |
|--------|-------------|----------|
| 共用 `load_and_validate()` | 单一函数：文件读取 + TOML 解析 + 语义验证；两个调用点共享 | ✓ |
| 分别实现 | install 和 validate 各自独立实现验证逻辑 | |

**Auto-selected:** 共用函数（recommended default）
**Notes:** DRY；避免日后修改验证规则时漏改一处。

---

## Claude's Discretion

- TOML 字段名沿用 InstallConfig snake_case 字段名（无需 serde rename）
- 语义验证错误用 `anyhow::bail!()` 而不新增 thiserror 类型
- `validate_install_config()` 设计为纯函数（便于单元测试）

## Deferred Ideas

- `[cluster]` TOML 段落 → Phase 3
- `--dry-run` 模式 → v2 需求
- 断点续传 → v2 需求
- 达梦自动下载 URL → 待 spike 验证
