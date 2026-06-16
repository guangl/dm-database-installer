---
quick_id: "260613-ttw"
status: complete
---

# Summary: 删除 Windows 安装支持

## Result

已移除所有 Windows 安装相关代码：

- `src/cli.rs`：删除 `InstallWindows` 枚举变体、`InstallWindowsArgs` 结构体和 2 个测试函数
- `src/main.rs`：删除 `InstallWindows` match 分支（含 PLAT-04 placeholder 注释）
- `Cargo.toml`：移除 `x86_64-pc-windows-msvc` 构建目标，`installers` 从 `["shell", "powershell"]` 改为 `["shell"]`

`cargo check` 和 `cargo test`（115 个测试）均通过，无回归。
