use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::cli::{ClusterInitKind, InitKind, InitOutputArgs};

pub fn run(kind: &InitKind) -> Result<()> {
    match kind {
        InitKind::Standalone(args) => {
            let dir = output_dir(args);
            write_template(&dir.join("config.toml"), args.force, STANDALONE_COMMON)?;
            write_template(&dir.join("standalone.toml"), args.force, STANDALONE_SPECIFIC)?;
            println!("已生成单机配置模板:");
            println!("  config.toml      — 通用配置（type、安装包路径等）");
            println!("  standalone.toml  — 单机特有配置（端口、路径、字符集等）");
            println!("编辑后使用: dm-installer install");
        }
        InitKind::Cluster(cluster_args) => run_cluster(&cluster_args.kind)?,
    }
    Ok(())
}

fn run_cluster(kind: &ClusterInitKind) -> Result<()> {
    match kind {
        ClusterInitKind::PrimaryStandby(args) => {
            let dir = output_dir(args);
            write_template(&dir.join("config.toml"), args.force, CLUSTER_COMMON)?;
            write_template(&dir.join("primary-standby.toml"), args.force, PS_SPECIFIC)?;
            println!("已生成主备集群配置模板:");
            println!("  config.toml            — 通用配置（type、安装包路径等）");
            println!("  primary-standby.toml   — 主备特有配置（节点、OGUID 等）");
            println!("编辑后使用: dm-installer install");
        }
        ClusterInitKind::Rws(args) => {
            let dir = output_dir(args);
            write_template(&dir.join("config.toml"), args.force, CLUSTER_COMMON_RWS)?;
            write_template(&dir.join("rws.toml"), args.force, RWS_SPECIFIC)?;
            println!("[占位] 已生成读写分离配置模板 (config.toml + rws.toml)");
            println!("注意: 读写分离部署逻辑尚未实现，模板仅供参考");
        }
        ClusterInitKind::Dsc(args) => {
            let dir = output_dir(args);
            write_template(&dir.join("config.toml"), args.force, CLUSTER_COMMON_DSC)?;
            write_template(&dir.join("dsc.toml"), args.force, DSC_SPECIFIC)?;
            println!("[占位] 已生成 DSC 集群配置模板 (config.toml + dsc.toml)");
            println!("注意: DSC 部署逻辑尚未实现，模板仅供参考");
        }
    }
    Ok(())
}

/// 返回输出目录：用户指定了 --output 时将其作为目录，否则用当前目录。
fn output_dir(args: &InitOutputArgs) -> PathBuf {
    args.output.clone().unwrap_or_else(|| PathBuf::from("."))
}

fn write_template(path: &Path, force: bool, content: &str) -> Result<()> {
    if path.exists() && !force {
        bail!("文件已存在: {}；使用 --force 强制覆盖", path.display());
    }
    std::fs::write(path, content)
        .map_err(|e| anyhow::anyhow!("无法写入配置文件 {}: {}", path.display(), e))
}

// ─── 模板内容 ──────────────────────────────────────────────────────────────

const STANDALONE_COMMON: &str = r#"# 达梦数据库单机安装 — 通用配置
# 使用方式: dm-installer install

type = "standalone"

# DM 安装包本地路径（不填则自动下载）
# installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

# 日志级别：trace / debug / info / warn / error
log_level = "info"

# SSH 默认凭证（单机安装无需填写）
# [ssh]
# user = "root"
# identity_file = "~/.ssh/id_rsa"
"#;

const STANDALONE_SPECIFIC: &str = r#"# 达梦数据库单机安装 — 特有配置（standalone.toml）
# 注意：SYSDBA / SYSAUDITOR 密码在安装时由终端提示输入，不写入此文件

# ─── 安装路径 ────────────────────────────────────────────────
install_path = "/home/dmdba/dmdbms"
data_path = "/home/dmdba/dmdbms/data"

# ─── 实例参数 ────────────────────────────────────────────────
instance_name = "DMSERVER"
port = 5236

# 页大小（KB），可选值：4 / 8 / 16 / 32
page_size = 32

# 字符集：0=GB18030  1=UTF-8  2=EUC-KR
charset = 1

case_sensitive = true

# 区段大小（页数），可选值：16 / 32
extent_size = 32

# ─── SSH 远程安装目标（可选）────────────────────────────────
# 填写后将通过 SSH 在目标服务器上安装，host 为本机时自动退化为本地安装。
# password 不填则运行时提示输入。
# [ssh_target]
# host = "192.168.1.100"
# ssh_port = 22
# user = "root"
# password = "your_ssh_password"
# max_retries = 3        # 连接失败最大重试次数，默认 3
# retry_interval_secs = 5  # 每次重试前等待秒数，默认 5
"#;

const CLUSTER_COMMON: &str = r#"# 达梦数据库主备集群 — 通用配置
# 使用方式: dm-installer install

type = "primary-standby"

# 控制机本地 DM 安装包路径（集群必填，会推送到各节点）
installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

log_level = "info"

# SSH 默认凭证（各节点未单独指定时使用）
[ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
# password = "your_password"
"#;

const CLUSTER_COMMON_RWS: &str = r#"# 达梦数据库读写分离集群 — 通用配置
# TODO: 读写分离部署逻辑尚未实现

type = "rws"

installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

log_level = "info"

[ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;

const CLUSTER_COMMON_DSC: &str = r#"# 达梦数据库 DSC 集群 — 通用配置
# TODO: DSC 部署逻辑尚未实现

type = "dsc"

installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

log_level = "info"

[ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;

const PS_SPECIFIC: &str = r#"# 达梦数据库主备集群 — 特有配置（primary-standby.toml）

# 守护系统全局唯一标识，主备节点必须相同，范围 0-2147483647
oguid = 453331

# ─── 主节点 ─────────────────────────────────────────────────
[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"
install_path = "/opt/dmdbms"
data_path = "/opt/dmdbms/data"
port = 5236
mal_port = 5237
dw_port = 5238
inst_dw_port = 5239
page_size = 8
charset = 0
case_sensitive = true
extent_size = 16

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
# password = "your_password"

# ─── 备节点 ─────────────────────────────────────────────────
[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"
install_path = "/opt/dmdbms"
data_path = "/opt/dmdbms/data"
port = 5236
mal_port = 5237
dw_port = 5238
inst_dw_port = 5239
page_size = 8
charset = 0
case_sensitive = true
extent_size = 16

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
# password = "your_password"
"#;

// TODO(cluster-rws): 读写分离部署逻辑待实现
const RWS_SPECIFIC: &str = r#"# 达梦数据库读写分离集群 — 特有配置（rws.toml）
# TODO: 读写分离部署逻辑尚未实现

oguid = 453331

# ─── 主节点（处理写入） ─────────────────────────────────────
[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"
install_path = "/opt/dmdbms"
data_path = "/opt/dmdbms/data"
port = 5236
mal_port = 5237
dw_port = 5238
inst_dw_port = 5239

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

# ─── 备节点（承担只读查询） ─────────────────────────────────
[[nodes]]
role = "standby"
read_only = true
host = "192.168.1.11"
instance_name = "DMSVR02"
install_path = "/opt/dmdbms"
data_path = "/opt/dmdbms/data"
port = 5236
mal_port = 5237
dw_port = 5238
inst_dw_port = 5239

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;

// TODO(cluster-dsc): DSC 共享存储集群部署逻辑待实现
const DSC_SPECIFIC: &str = r#"# 达梦数据库 DSC 共享存储集群 — 特有配置（dsc.toml）
# TODO: DSC 部署逻辑尚未实现

oguid = 453331
shared_storage = "/dev/sdc"

# ─── 节点 1（负责初始化共享实例） ──────────────────────────
[[nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"
install_path = "/opt/dmdbms"
data_path = "/dmdata/shared"
port = 5236
mal_port = 5237
dw_port = 5238
inst_dw_port = 5239

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

# ─── 节点 2 ─────────────────────────────────────────────────
[[nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"
install_path = "/opt/dmdbms"
data_path = "/dmdata/shared"
port = 5236
mal_port = 5237
dw_port = 5238
inst_dw_port = 5239

[nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn output_args_in(dir: &TempDir, force: bool) -> InitOutputArgs {
        InitOutputArgs { output: Some(dir.path().to_path_buf()), force }
    }

    fn cluster_args(kind: ClusterInitKind) -> InitKind {
        InitKind::Cluster(crate::cli::ClusterInitArgs { kind })
    }

    #[test]
    fn test_standalone_creates_two_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        assert!(dir.path().join("config.toml").exists(), "应生成 config.toml");
        assert!(dir.path().join("standalone.toml").exists(), "应生成 standalone.toml");
    }

    #[test]
    fn test_standalone_common_has_type_field() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(content.contains("type = \"standalone\""), "通用配置应含 type = \"standalone\"");
    }

    #[test]
    fn test_standalone_specific_has_install_fields() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("standalone.toml")).unwrap();
        assert!(content.contains("install_path"), "特有配置应含 install_path");
        assert!(content.contains("port = 5236"), "特有配置应含默认端口");
    }

    #[test]
    fn test_standalone_templates_are_valid_toml() {
        toml::from_str::<toml::Value>(STANDALONE_COMMON).expect("通用模板应为合法 TOML");
        toml::from_str::<toml::Value>(STANDALONE_SPECIFIC).expect("单机特有模板应为合法 TOML");
    }

    #[test]
    fn test_cluster_ps_creates_two_files() {
        let dir = TempDir::new().unwrap();
        let kind = cluster_args(ClusterInitKind::PrimaryStandby(output_args_in(&dir, false)));
        run(&kind).unwrap();
        assert!(dir.path().join("config.toml").exists(), "应生成 config.toml");
        assert!(dir.path().join("primary-standby.toml").exists(), "应生成 primary-standby.toml");
    }

    #[test]
    fn test_cluster_ps_common_has_correct_type() {
        let dir = TempDir::new().unwrap();
        let kind = cluster_args(ClusterInitKind::PrimaryStandby(output_args_in(&dir, false)));
        run(&kind).unwrap();
        let content = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(content.contains("type = \"primary-standby\""), "通用配置应含正确的 type");
    }

    #[test]
    fn test_cluster_ps_specific_has_nodes() {
        let dir = TempDir::new().unwrap();
        let kind = cluster_args(ClusterInitKind::PrimaryStandby(output_args_in(&dir, false)));
        run(&kind).unwrap();
        let content = std::fs::read_to_string(dir.path().join("primary-standby.toml")).unwrap();
        assert!(content.contains("role = \"primary\""), "特有配置应含 primary 节点");
        assert!(content.contains("role = \"standby\""), "特有配置应含 standby 节点");
    }

    #[test]
    fn test_cluster_rws_creates_two_files() {
        let dir = TempDir::new().unwrap();
        let kind = cluster_args(ClusterInitKind::Rws(output_args_in(&dir, false)));
        run(&kind).unwrap();
        assert!(dir.path().join("config.toml").exists());
        assert!(dir.path().join("rws.toml").exists());
        let content = std::fs::read_to_string(dir.path().join("rws.toml")).unwrap();
        assert!(content.contains("read_only = true"));
    }

    #[test]
    fn test_cluster_dsc_creates_two_files() {
        let dir = TempDir::new().unwrap();
        let kind = cluster_args(ClusterInitKind::Dsc(output_args_in(&dir, false)));
        run(&kind).unwrap();
        assert!(dir.path().join("config.toml").exists());
        assert!(dir.path().join("dsc.toml").exists());
        let content = std::fs::read_to_string(dir.path().join("dsc.toml")).unwrap();
        assert!(content.contains("shared_storage"));
    }

    #[test]
    fn test_refuses_to_overwrite_without_force() {
        let dir = TempDir::new().unwrap();
        // 先生成一次
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        // 再生成应该因为 config.toml 已存在而报错
        let err = run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap_err();
        assert!(format!("{err}").contains("文件已存在"));
    }

    #[test]
    fn test_force_overwrites_existing_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, true))).unwrap();
        assert!(dir.path().join("standalone.toml").exists());
    }

    #[test]
    fn test_all_templates_are_valid_toml() {
        for (name, tmpl) in [
            ("STANDALONE_COMMON", STANDALONE_COMMON),
            ("STANDALONE_SPECIFIC", STANDALONE_SPECIFIC),
            ("CLUSTER_COMMON", CLUSTER_COMMON),
            ("PS_SPECIFIC", PS_SPECIFIC),
            ("RWS_SPECIFIC", RWS_SPECIFIC),
            ("DSC_SPECIFIC", DSC_SPECIFIC),
        ] {
            toml::from_str::<toml::Value>(tmpl)
                .unwrap_or_else(|e| panic!("{name} 应为合法 TOML: {e}"));
        }
    }
}
