# 安装

## 系统要求

| 平台 | 架构 | 最低版本 |
|------|------|----------|
| Linux | x86_64 | glibc ≥ 2.23（Ubuntu 16.04 / CentOS 7 / Debian 8 及以上） |
| Linux | aarch64 | glibc ≥ 2.23 |
| macOS | x86_64 | 10.12+ |
| macOS | Apple Silicon | 11.0+ |
| Windows | x86_64 | Windows 10 / Server 2016+ |

---

## 方式一：一行命令直接安装 DM8（开发者推荐）

纯 shell 脚本，零依赖，无需安装任何工具：

```sh
curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh | bash
```

脚本完成后会打印凭证卡片，例如：

```
╔════════════════════════════════════════╗
║         DM8 安装完成                   ║
╠════════════════════════════════════════╣
║  SYSDBA 密码:    Dm#aB3kx9P           ║
║  SYSAUDITOR 密码: Gx7@mNrQ2L          ║
║                                        ║
║  连接命令:                             ║
║    disql SYSDBA/Dm#aB3kx9P@localhost   ║
╚════════════════════════════════════════╝
```

**请立即保存密码**，脚本不会再次显示。

> 仅支持 Linux（x86_64 / aarch64）。macOS / Windows 用户请使用方式二或三安装 `dm-installer` 工具。

---

## 方式二：安装 dm-installer 工具（自定义 / 集群场景）

适合需要自定义参数、SSH 远程部署、主备集群等精细化场景。

### Linux / macOS

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/guangl/dm-database-installer/releases/latest/download/dm-database-installer-installer.sh | sh
```

安装完成后**重新打开终端**或执行：

```sh
source ~/.cargo/env
```

验证安装：

```sh
dm-installer --version
```

### Windows（PowerShell）

```powershell
powershell -ExecutionPolicy Bypass -c `
  "irm https://github.com/guangl/dm-database-installer/releases/latest/download/dm-database-installer-installer.ps1 | iex"
```

重新打开 PowerShell 后验证：

```powershell
dm-installer --version
```

---

## 方式三：从 GitHub Releases 手动下载

访问 [Releases 页面](https://github.com/guangl/dm-database-installer/releases)，下载对应平台的压缩包：

| 文件名 | 平台 |
|--------|------|
| `dm-installer-x86_64-unknown-linux-gnu.tar.gz` | Linux x86_64 |
| `dm-installer-aarch64-unknown-linux-gnu.tar.gz` | Linux ARM64 |
| `dm-installer-x86_64-apple-darwin.tar.gz` | macOS Intel |
| `dm-installer-aarch64-apple-darwin.tar.gz` | macOS Apple Silicon |
| `dm-installer-x86_64-pc-windows-msvc.zip` | Windows x86_64 |

解压后将 `dm-installer`（Windows 为 `dm-installer.exe`）放入 `$PATH` 中的任意目录。

---

## 方式四：从 crates.io 安装

需要已安装 Rust toolchain（[rustup.rs](https://rustup.rs)）：

```sh
cargo install dm-database-installer
```

---

## 方式五：从源码编译

```sh
git clone https://github.com/guangl/dm-database-installer.git
cd dm-database-installer
cargo build --release
# 二进制位于 target/release/dm-installer
```

---

## Shell 补全（可选）

安装 `dm-installer` 后可生成 shell 补全脚本：

```sh
# Bash
dm-installer completions bash >> ~/.bash_completion

# Zsh（将 ~/.zfunc 加入 fpath 后执行 compinit）
dm-installer completions zsh > ~/.zfunc/_dm-installer

# Fish
dm-installer completions fish > ~/.config/fish/completions/dm-installer.fish
```
