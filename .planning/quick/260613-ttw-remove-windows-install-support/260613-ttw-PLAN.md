---
quick_id: "260613-ttw"
slug: "remove-windows-install-support"
description: "删除 Windows 安装支持"
date: "2026-06-13"
---

# Quick Task: 删除 Windows 安装支持

## Goal

移除项目中所有 Windows 安装相关代码，聚焦 Linux 平台。

## Changes

1. `src/cli.rs` — 移除 `InstallWindows` 命令变体、`InstallWindowsArgs` 结构体、两个 Windows 测试
2. `src/main.rs` — 移除 `InstallWindows` match 分支
3. `Cargo.toml` — 移除 `x86_64-pc-windows-msvc` 构建目标和 `powershell` installer

## Must Haves

- [ ] `cargo check` 无错误
- [ ] `cargo test` 全部通过
- [ ] 无 InstallWindows 残留引用
