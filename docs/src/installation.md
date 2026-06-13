# 安装

## 系统要求

| 平台 | 架构 | 最低 glibc |
|------|------|-----------|
| Linux | x86_64 | 2.28（CentOS 8 / RHEL 8 / Debian 10 以上） |
| Linux | aarch64 | 2.17 |
| Windows | x86_64 | — |

## 方式一：预编译二进制（推荐）

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

## 方式二：从 crates.io 安装

需要已安装 Rust toolchain（[rustup.rs](https://rustup.rs)）。

```sh
cargo install dm-database-installer
```

## 方式三：从 GitHub Releases 手动下载

访问 [Releases 页面](https://github.com/guangl/dm-database-installer/releases) 下载对应平台的压缩包，解压后将 `dm-installer` 放入 `$PATH` 中的任意目录。

## 方式四：从源码编译

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
