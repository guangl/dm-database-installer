# 安装

## 系统要求

| 平台 | 架构 | 备注 |
|------|------|------|
| Linux | x86_64 | musl 静态链接，无 glibc 依赖 |
| Linux | aarch64 | musl 静态链接，无 glibc 依赖 |
| macOS | Apple Silicon | 11.0+ |

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

> 仅支持 Linux（x86_64 / aarch64）。macOS 用户请使用方式二或三安装 `dm-installer` 工具。

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

---

## 方式三：从 GitHub Releases 手动下载

访问 [Releases 页面](https://github.com/guangl/dm-database-installer/releases)，下载对应平台的压缩包：

| 文件名 | 平台 |
|--------|------|
| `dm-installer-x86_64-unknown-linux-musl.tar.gz` | Linux x86_64 |
| `dm-installer-aarch64-unknown-linux-musl.tar.gz` | Linux ARM64 |
| `dm-installer-aarch64-apple-darwin.tar.gz` | macOS Apple Silicon |

解压后将 `dm-installer` 放入 `$PATH` 中的任意目录。

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

