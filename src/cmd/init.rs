use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::cli::{InitKind, InitOutputArgs};

pub fn run(kind: &InitKind) -> Result<()> {
    match kind {
        InitKind::Standalone(args) => {
            let dir = output_dir(args);
            let wrote_common =
                write_template(&dir.join("config.toml"), args.force, STANDALONE_COMMON)?;
            let wrote_specific = write_template(
                &dir.join("standalone.toml"),
                args.force,
                STANDALONE_SPECIFIC,
            )?;
            if wrote_common || wrote_specific {
                println!("已生成单机配置模板:");
                if wrote_common {
                    println!("  config.toml      — 通用配置（type、安装包路径等）");
                }
                if wrote_specific {
                    println!("  standalone.toml  — 单机特有配置（端口、路径、字符集等）");
                }
                println!("编辑后使用: dm-installer install");
            } else {
                println!("配置文件已存在，无需覆盖。使用 --force 强制重新生成。");
            }
            Ok(())
        }
        InitKind::Dw | InitKind::Rws | InitKind::Dsc | InitKind::Dpc => {
            let mode = match kind {
                InitKind::Dw => "主备集群（dw）",
                InitKind::Rws => "读写分离集群（rws）",
                InitKind::Dsc => "DSC 共享存储集群（dsc）",
                InitKind::Dpc => "DPC 分布式集群（dpc）",
                _ => unreachable!(),
            };
            println!("{} 配置模板即将支持，请关注后续版本。", mode);
            println!("当前可使用: dm-installer init standalone");
            Ok(())
        }
    }
}

fn output_dir(args: &InitOutputArgs) -> PathBuf {
    args.output.clone().unwrap_or_else(|| PathBuf::from("."))
}

/// 返回 true 表示实际写入了文件，false 表示跳过（已存在且未 force）
fn write_template(path: &Path, force: bool, content: &str) -> Result<bool> {
    if path.exists() && !force {
        println!("跳过已存在的文件: {}", path.display());
        return Ok(false);
    }
    std::fs::write(path, content)
        .map_err(|e| anyhow::anyhow!("无法写入配置文件 {}: {}", path.display(), e))?;
    Ok(true)
}

const STANDALONE_COMMON: &str = r#"# 达梦数据库单机安装 — 通用配置
# 使用方式: dm-installer install

type = "standalone"

# ─── 安装包来源（三选一，都不填则自动检测下载）────────────────
# 本地文件路径
# installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"
# 自定义下载链接
# installer_url = "https://download.example.com/dm8.zip"
"#;

const STANDALONE_SPECIFIC: &str = r#"# 达梦数据库单机安装 — 特有配置（standalone.toml）
# 注意：SYSDBA / SYSAUDITOR 密码在安装时由终端提示输入，不写入此文件

[install]
install_path = "/home/dmdba/dmdbms"
data_path = "/home/dmdba/dmdbms/data"

[instance]
instance_name = "DMSERVER"
port = 5236
ap_port = 4236
# 页大小（KB），可选值：4 / 8 / 16 / 32
page_size = 32
# 字符集：0=GB18030  1=UTF-8  2=EUC-KR
charset = 1
case_sensitive = true
# 区段大小（页数），可选值：16 / 32
extent_size = 32

# ─── 本地归档配置 ──────────────────────────────────────────
# 单机模式默认开启本地归档（ARCH_INI=1），以下参数均可省略走默认值。
[archive]
# arch_path = "/home/dmdba/dmdbms/data/arch"  # 不填则默认为 data_path/arch
file_size   = 128   # 单归档文件大小（MB）
space_limit = 0     # 归档空间上限（MB），0 = 无限
hang_flag   = false # 归档失败时是否挂起数据库（单机建议 false）
compressed  = false # 是否压缩归档文件

# ─── SSH 远程安装目标（可选）────────────────────────────────
# 填写后将通过 SSH 在目标服务器上安装，host 为本机时自动退化为本地安装。
# password 不填则运行时提示输入。
# [ssh_target]
# host = "192.168.1.100"
# ssh_port = 22
# user = "root"
# password = "your_ssh_password"
# max_retries = 3
# retry_interval_secs = 5
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn output_args_in(dir: &TempDir, force: bool) -> InitOutputArgs {
        InitOutputArgs {
            output: Some(dir.path().to_path_buf()),
            force,
        }
    }

    #[test]
    fn test_standalone_creates_two_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        assert!(
            dir.path().join("config.toml").exists(),
            "应生成 config.toml"
        );
        assert!(
            dir.path().join("standalone.toml").exists(),
            "应生成 standalone.toml"
        );
    }

    #[test]
    fn test_standalone_common_has_type_field() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(
            content.contains("type = \"standalone\""),
            "通用配置应含 type = \"standalone\""
        );
    }

    #[test]
    fn test_standalone_specific_has_install_fields() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("standalone.toml")).unwrap();
        assert!(
            content.contains("install_path"),
            "特有配置应含 install_path"
        );
        assert!(content.contains("port = 5236"), "特有配置应含默认端口");
    }

    #[test]
    fn test_standalone_templates_are_valid_toml() {
        toml::from_str::<toml::Value>(STANDALONE_COMMON).expect("通用模板应为合法 TOML");
        toml::from_str::<toml::Value>(STANDALONE_SPECIFIC).expect("单机特有模板应为合法 TOML");
    }

    #[test]
    fn test_skips_existing_files_without_force() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        // 第二次运行不报错，只跳过已存在文件
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
    }

    #[test]
    fn test_partial_init_creates_missing_file() {
        let dir = TempDir::new().unwrap();
        // 只预先创建 config.toml
        std::fs::write(dir.path().join("config.toml"), "type = \"standalone\"\n").unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        // standalone.toml 应该被创建
        assert!(
            dir.path().join("standalone.toml").exists(),
            "standalone.toml 应被创建"
        );
    }

    #[test]
    fn test_force_overwrites_existing_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, true))).unwrap();
        assert!(dir.path().join("standalone.toml").exists());
    }
}
