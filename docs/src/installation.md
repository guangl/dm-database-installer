# 安装

## 系统要求

| 平台 | 架构 | 备注 |
|------|------|------|
| Linux | x86_64 | glibc ≥ 2.23（Ubuntu 16.04 / CentOS 7 / Debian 8 以上） |
| Linux | aarch64 | glibc ≥ 2.23 |
| macOS | x86_64 | 10.12+ |
| macOS | Apple Silicon | 11.0+ |
| Windows | x86_64 | — |

## 方式一：一行命令直接安装 DM 数据库（开发者推荐）

纯 shell 脚本，无需 Rust，无需任何外部依赖：

```sh
curl -fsSL https://raw.githubusercontent.com/guangl/dm-database-installer/main/install.sh | bash
```

脚本会自动检测当前系统架构和发行版，下载对应的 DM8 安装包并完成静默安装。安装完成后会输出随机生成的 SYSDBA / SYSAUDITOR 密码，请妥善保存。

> 仅支持 Linux（x86_64 / aarch64）。Windows 用户请使用方式二或三。

## 方式二：安装 dm-installer 管理工具

适合需要自定义安装参数、SSH 远程部署、主备集群等精细化场景。

### Linux / macOS

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/guangl/dm-database-installer/releases/latest/download/dm-database-installer-installer.sh | sh
```

安装后重新打开终端，或执行 `source ~/.cargo/env`。验证安装：

```sh
dm-installer --version
```

### Windows（PowerShell）

```powershell
powershell -ExecutionPolicy Bypass -c `
  "irm https://github.com/guangl/dm-database-installer/releases/latest/download/dm-database-installer-installer.ps1 | iex"
```

## 方式三：从 crates.io 安装

需要已安装 Rust toolchain（[rustup.rs](https://rustup.rs)）。

```sh
cargo install dm-database-installer
```

## 方式四：从 GitHub Releases 手动下载

访问 [Releases 页面](https://github.com/guangl/dm-database-installer/releases) 下载对应平台的压缩包，解压后将 `dm-installer` 放入 `$PATH` 中的任意目录。

## 方式五：从源码编译

```sh
git clone https://github.com/guangl/dm-database-installer.git
cd dm-database-installer
cargo build --release
# 二进制：target/release/dm-installer
```

## Shell 补全（可选）

```sh
# Bash
dm-installer completions bash >> ~/.bash_completion

# Zsh
dm-installer completions zsh > ~/.zfunc/_dm-installer
# 确保 ~/.zshrc 中有 fpath=(~/.zfunc $fpath) 和 autoload -U compinit && compinit

# Fish
dm-installer completions fish > ~/.config/fish/completions/dm-installer.fish
```
