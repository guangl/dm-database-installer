---
phase: "04-release"
plan: "02"
subsystem: "release-pipeline"
tags:
  - rust
  - release
  - ci
  - cargo-dist

dependency_graph:
  requires:
    - "04-01: Cargo.toml description/license/repository 字段"
  provides:
    - "Cargo.toml: [workspace.metadata.dist] 三平台 targets + aarch64 apt 依赖 + [profile.dist]"
    - ".cargo/config.toml: aarch64-linux-gnu-gcc linker 配置"
    - ".github/workflows/release.yml: v* tag 触发 + 动态 matrix + NASM 步骤"
    - "src/cli.rs: InstallWindows(InstallWindowsArgs) + InstallWindowsArgs 结构体（PLAT-04）"
    - "src/main.rs: Commands::InstallWindows match 分支（eprintln + exit 1）"
  affects:
    - "04-03: tag push v0.1.0 触发 Release workflow；workflow name = 'Release'"

tech_stack:
  added:
    - "cargo-dist 0.32.0 (dev tool, not runtime dependency)"
  patterns:
    - "cargo-dist [workspace.metadata.dist] 配置块写入 Cargo.toml"
    - "allow-dirty=[ci] 允许 release.yml 含手动添加的 NASM 步骤"
    - "dist plan 动态 matrix: fromJson(needs.plan.outputs.val).ci.github.artifacts_matrix"
    - "clap derive InstallWindows(InstallWindowsArgs) + eprintln+exit 1 (D-07 方案)"

key_files:
  created:
    - path: ".cargo/config.toml"
      changes: "aarch64-unknown-linux-gnu linker = aarch64-linux-gnu-gcc"
    - path: ".github/workflows/release.yml"
      changes: "dist init 生成 + 手动追加 NASM 步骤 (build-local-artifacts job step 139-142)"
  modified:
    - path: "Cargo.toml"
      changes: "[workspace.metadata.dist] 块 + [profile.dist] + allow-dirty=[ci]"
    - path: "src/cli.rs"
      changes: "Commands::InstallWindows(InstallWindowsArgs) variant + InstallWindowsArgs struct + 2 tests"
    - path: "src/main.rs"
      changes: "Commands::InstallWindows match branch: eprintln + std::process::exit(1)"

decisions:
  - "D-04（已授权 2026-06-13）: Windows target 使用 x86_64-pc-windows-msvc 替代 x86_64-pc-windows-gnu（ring#1363 + mio#1632 阻断 windows-gnu）"
  - "D-05（已授权 2026-06-13）: 使用 apt 安装 gcc-aarch64-linux-gnu + .cargo/config.toml linker，不使用 cross 容器"
  - "D-07（已授权 2026-06-13）: PLAT-04 placeholder 使用 eprintln + exit(1) 替代 todo!()，用户体验更友好"
  - "allow-dirty=[ci] 配置: 让 dist plan 忽略 release.yml 的 NASM 步骤差异（dist generate 会删除手动追加内容）"
  - "dist-workspace.toml: dist init 生成独立文件，已删除并将配置迁移到 Cargo.toml [workspace.metadata.dist]"

metrics:
  duration: "~25 minutes"
  completed_date: "2026-06-13T12:23:23Z"
  tasks_completed: 2
  files_changed: 5
  tests_added: 2
  tests_modified: 0
---

# Phase 04 Plan 02: cargo-dist 配置 + PLAT-04 CLI Placeholder Summary

**一句话：** cargo-dist 0.32.0 配置三平台构建矩阵（linux-x86/aarch64/windows-msvc），生成含 NASM 步骤的 GitHub Actions release.yml，并添加 PLAT-04 InstallWindows placeholder CLI。

## 任务完成状态

| Task | 名称 | 状态 | 提交 |
|------|------|------|------|
| 1 (TDD RED) | PLAT-04 InstallWindows 失败测试 | 完成 | 926216f |
| 1 (TDD GREEN) | PLAT-04 InstallWindows 实现 | 完成 | c2bb428 |
| 2 | cargo-dist 配置 + .cargo/config.toml + release.yml | 完成 | dedbe46 |

## CONTEXT 决策覆盖记录（三项，均于 2026-06-13 获用户授权）

### D-04：Windows 目标 windows-gnu → windows-msvc

**授权时间：** 2026-06-13（04-CONTEXT.md L33 已更新）

**原决策：** Windows 目标 `x86_64-pc-windows-gnu`

**更新后：** Windows 目标 `x86_64-pc-windows-msvc`

**理由：** RESEARCH.md（HIGH 置信度）确认 ring#1363 + mio#1632 双重不兼容，windows-gnu 在 Linux runner 交叉编译无法产出有效二进制；windows-msvc 在 windows-2022 runner 原生构建已被 russh CI 验证可行。

**实际执行：** Cargo.toml targets 数组使用 `"x86_64-pc-windows-msvc"`；release.yml 中 Windows 构建运行在 windows-2022 runner（由 dist plan 动态决定）。

### D-05：交叉编译 cross 容器 → apt + linker 方案

**授权时间：** 2026-06-13（04-CONTEXT.md L37 已更新）

**原决策：** 使用 `cross` 工具链 Docker-based 交叉编译

**更新后：** apt 安装 `gcc-aarch64-linux-gnu` + `.cargo/config.toml` linker 配置

**理由：** cargo-dist 不调用 cross，apt 工具链方案更直接；功能等价。

**实际执行：**
- Cargo.toml `[workspace.metadata.dist.dependencies.apt]` 声明 `gcc-aarch64-linux-gnu`
- Cargo.toml `[workspace.metadata.dist.github-custom-runners]` 指定 aarch64 用 ubuntu-22.04
- `.cargo/config.toml` 配置 `[target.aarch64-unknown-linux-gnu] linker = "aarch64-linux-gnu-gcc"`

### D-07：PLAT-04 placeholder todo!() → eprintln + exit(1)

**授权时间：** 2026-06-13（04-02-PLAN.md frontmatter context_overrides D-07 条目）

**原决策：** PLAT-04 placeholder 使用 `todo!()` + 文档注释

**更新后：** `eprintln!` + `std::process::exit(1)`

**理由：** 避免 panic backtrace；用户看到明确的中文错误信息；功能等价（CLI 入口存在、exit 1、不实际安装）。

**PLAT-04 placeholder stderr 文案：**
```
[WARN] Windows 目标机安装尚未实现（PLAT-04 spike 待完成）
请参考: https://eco.dameng.com/ 手动获取 Windows 安装包
```

## cargo-dist 安装和配置

**安装命令：** `cargo install cargo-dist --version 0.32.0 --locked`

**安装位置：** `~/.cargo/bin/dist`（注意：命令名是 `dist`，不是 `cargo dist`）

**版本输出：** `cargo-dist 0.32.0`

**dist init 执行：**
```bash
dist init --yes --hosting=github --installer=shell --installer=powershell --ci=github
```

dist init 将配置写入了 `dist-workspace.toml`（独立文件），并生成了 `.github/workflows/release.yml`。

**手动调整：**
1. 删除 `dist-workspace.toml`
2. 将配置迁移到 `Cargo.toml` 的 `[workspace.metadata.dist]`（Plan 要求位置）
3. 调整 targets 为三平台（dist init 默认生成了 5 个平台含两个 Apple Silicon）
4. 添加 aarch64 custom runner 和 apt 依赖
5. 添加 `allow-dirty = ["ci"]` 以允许手动添加的 NASM 步骤

## release.yml 关键结构

**NASM 步骤精确位置：** `build-local-artifacts` job 的 steps 列表中，在 `Install dependencies` 步骤之前（第 139-142 行）：

```yaml
      - name: Install NASM (Windows)
        if: runner.os == 'Windows'
        run: choco install nasm -y && echo "C:\Program Files\NASM" >> $env:GITHUB_PATH
        shell: pwsh
```

**build-local-artifacts matrix 实际形式（动态展开，非静态写死）：**

```yaml
build-local-artifacts:
  needs:
    - plan
  strategy:
    fail-fast: false
    matrix: ${{ fromJson(needs.plan.outputs.val).ci.github.artifacts_matrix }}
  runs-on: ${{ matrix.runner }}
```

matrix 由 `plan` job 的输出动态展开（`fromJson(needs.plan.outputs.val).ci.github.artifacts_matrix`），未静态写死三个 triple。

## cargo dist plan 实际输出（验证三平台）

```
announcing v0.1.0
  dm-database-installer 0.1.0
    source.tar.gz
    dm-database-installer-installer.sh
    dm-database-installer-installer.ps1
    sha256.sum
    dm-database-installer-aarch64-unknown-linux-gnu.tar.xz  ← PLAT-02
      [bin] dm-installer
    dm-database-installer-x86_64-pc-windows-msvc.zip       ← PLAT-03 (D-04: msvc 非 gnu)
      [bin] dm-installer.exe
    dm-database-installer-x86_64-unknown-linux-gnu.tar.xz  ← PLAT-01
      [bin] dm-installer
```

三个平台全部命中：`x86_64-unknown-linux-gnu` / `aarch64-unknown-linux-gnu` / `x86_64-pc-windows-msvc`。

## PLAT 需求覆盖状态

| 需求 | 描述 | 覆盖方式 | 状态 |
|------|------|---------|------|
| PLAT-01 | linux x86_64 | Cargo.toml targets + dist plan 输出 | 完成 |
| PLAT-02 | aarch64 linux | Cargo.toml targets + aarch64 apt 依赖 + .cargo/config.toml linker | 完成 |
| PLAT-03 | windows x86_64 (msvc) | Cargo.toml targets + NASM step | 完成 |
| PLAT-04 | install-windows CLI | src/cli.rs InstallWindows + src/main.rs eprintln+exit 1 | 完成 (placeholder) |

## 给 Plan 03 的接口

**触发 workflow 的 tag 格式：** `v0.1.0`（满足 `**[0-9]+.[0-9]+.[0-9]+*` 规则）

**触发的 workflow 名：** `Release`（`.github/workflows/release.yml` 中 `name: Release`）

**aarch64 linker 配置可见性：** dist plan 输出中 `dm-database-installer-aarch64-unknown-linux-gnu.tar.xz` 确认 aarch64 target 已被识别；CI 中 apt 安装 `gcc-aarch64-linux-gnu` 后 `.cargo/config.toml` 的 linker 配置会生效。

**验证命令：** 推送 `v0.1.0` tag 后，在 GitHub Actions 查看 `Release` workflow 是否触发并构建三平台二进制。

## 新增测试

| 测试名 | 位置 | 验证内容 |
|--------|------|---------|
| `test_install_windows_placeholder_parses` | src/cli.rs | install-windows 无参数解析为 InstallWindows，config 为 None |
| `test_install_windows_with_config` | src/cli.rs | install-windows --config 解析为 Some(PathBuf) |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] dist init 生成 dist-workspace.toml 而非写入 Cargo.toml**

- **发现于：** Task 2 步骤 2（运行 dist init）
- **问题：** dist 0.32.0 将配置写入独立的 `dist-workspace.toml` 文件，而 Plan 要求配置在 `Cargo.toml [workspace.metadata.dist]`
- **修复：** 删除 `dist-workspace.toml`，手动将配置迁移到 `Cargo.toml`（按 PATTERNS.md 模板）
- **影响：** 功能等价，cargo-dist 优先读取 Cargo.toml 中的 `[workspace.metadata.dist]`

**2. [Rule 3 - Blocking] dist plan 因 release.yml 手动修改报 out-of-date 错误**

- **发现于：** Task 2 步骤 6（运行 cargo dist plan 验证）
- **问题：** 手动追加 NASM 步骤后，dist plan 检测到 release.yml 与期望内容不同并报错
- **修复：** 在 `[workspace.metadata.dist]` 添加 `allow-dirty = ["ci"]`，让 dist 忽略 CI 文件的差异检查
- **影响：** dist plan 成功执行，功能等价

## Known Stubs

**PLAT-04 InstallWindows：** `src/main.rs` 中的 `Commands::InstallWindows` 分支是明确的 placeholder，执行 `eprintln! + std::process::exit(1)`。这是 CONTEXT.md D-07 授权的设计——CLI 入口已存在，真实 `setup.exe /q /XML <path>` 集成留给后续 spike。

## Self-Check: PASSED

- src/cli.rs 含 InstallWindows variant + InstallWindowsArgs：已确认（第 26 行、第 90 行）
- src/main.rs 含 InstallWindows match 分支（eprintln + exit 1）：已确认（第 32 行）
- Cargo.toml 含 [workspace.metadata.dist]：已确认
- .cargo/config.toml 存在含 aarch64-linux-gnu-gcc：已确认
- .github/workflows/release.yml 存在含 tags/NASM/fromJson：已确认
- 三提交存在：926216f (RED), c2bb428 (GREEN), dedbe46 (Task 2)
- cargo test 全绿：89 passed; 0 failed
- cargo build --release 通过
- dist plan 成功输出三平台构建矩阵
