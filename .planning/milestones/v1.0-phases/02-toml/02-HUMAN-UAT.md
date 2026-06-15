---
status: partial
phase: 02-toml
source: [02-VERIFICATION.md]
started: 2026-06-12T10:30:00Z
updated: 2026-06-12T10:30:00Z
---

## Current Test

[awaiting human testing]

## Tests

### 1. INST-02 SC1/SC2 完整安装流程

expected: 运行 `dm-installer install --config dm.toml --package /path/to/dm.iso` 时，安装器读取 TOML 配置并按参数执行完整安装（dminit 产出的 dm.ini 中 PORT/PAGE_SIZE/CHARSET 与 TOML 值一致）
result: [pending]

### 2. 参数确认 UI 展示 TOML 值

expected: 运行 `dm-installer install --config tests/fixtures/valid.toml`（不带 --yes/--defaults），交互式终端显示的 PAGE_SIZE/CHARSET/CASE_SENSITIVE/EXTENT_SIZE 值与 TOML 文件中的值一致，而非硬编码默认值
result: [pending]

## Summary

total: 2
passed: 0
issues: 0
pending: 2
skipped: 0
blocked: 0

## Gaps
