use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::cli::{ClusterInitKind, InitKind, InitOutputArgs};

pub fn run(kind: &InitKind) -> Result<()> {
    match kind {
        InitKind::Standalone(args) => {
            let path = resolve_output(args, "dm-standalone.toml");
            write_template(&path, args.force, STANDALONE_TEMPLATE)?;
            println!("已生成单机配置模板: {}", path.display());
            println!("编辑后使用: dm-installer install --config {}", path.display());
        }
        InitKind::Cluster(cluster_args) => run_cluster(&cluster_args.kind)?,
    }
    Ok(())
}

fn run_cluster(kind: &ClusterInitKind) -> Result<()> {
    match kind {
        ClusterInitKind::PrimaryStandby(args) => {
            let path = resolve_output(args, "dm-cluster-ps.toml");
            write_template(&path, args.force, CLUSTER_PS_TEMPLATE)?;
            println!("已生成主备集群配置模板: {}", path.display());
            println!("编辑后使用: dm-installer cluster deploy --config {}", path.display());
        }
        ClusterInitKind::Rws(args) => {
            emit_placeholder(args, "dm-cluster-rws.toml", "读写分离", CLUSTER_RWS_PLACEHOLDER)?;
        }
        ClusterInitKind::Dsc(args) => {
            emit_placeholder(args, "dm-cluster-dsc.toml", "DSC 共享存储集群", CLUSTER_DSC_PLACEHOLDER)?;
        }
    }
    Ok(())
}

fn emit_placeholder(args: &InitOutputArgs, default_name: &str, label: &str, template: &str) -> Result<()> {
    let path = resolve_output(args, default_name);
    write_template(&path, args.force, template)?;
    println!("[占位] 已生成 {} 配置模板: {}", label, path.display());
    println!("注意: {} 部署逻辑尚未实现，模板仅供参考", label);
    Ok(())
}

fn resolve_output(args: &InitOutputArgs, default_name: &str) -> PathBuf {
    args.output.clone().unwrap_or_else(|| PathBuf::from(default_name))
}

fn write_template(path: &Path, force: bool, content: &str) -> Result<()> {
    if path.exists() && !force {
        bail!("文件已存在: {}；使用 --force 强制覆盖", path.display());
    }
    std::fs::write(path, content)
        .map_err(|e| anyhow::anyhow!("无法写入配置文件 {}: {}", path.display(), e))
}

const STANDALONE_TEMPLATE: &str = r#"# 达梦数据库单机安装配置
# 使用方式: dm-installer install --config dm-standalone.toml
# 注意：SYSDBA / SYSAUDITOR 密码在安装时由终端提示输入，不写入此文件

# ─── 安装路径 ────────────────────────────────────────────────

# DM 安装根目录
install_path = "/home/dmdba/dmdbms"

# 数据文件目录
data_path = "/home/dmdba/dmdbms/data"

# ─── 实例参数 ────────────────────────────────────────────────

# 数据库实例名
instance_name = "DMSERVER"

# 监听端口
port = 5236

# 页大小（KB），可选值：4 / 8 / 16 / 32
page_size = 32

# 字符集：0=GB18030  1=UTF-8  2=EUC-KR
charset = 1

# 大小写敏感
case_sensitive = true

# 区段大小（页数），可选值：16 / 32
extent_size = 32
"#;

const CLUSTER_PS_TEMPLATE: &str = r#"# 达梦数据库主备集群安装配置
# 使用方式: dm-installer cluster deploy --config dm-cluster-ps.toml

[cluster]
type = "primary-standby"

# 控制机本地安装包路径
installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"

# 守护系统全局唯一标识，主备节点必须相同，范围 0-2147483647
oguid = 453331

# ─── 主节点 ─────────────────────────────────────────────
[[cluster.nodes]]
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

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
# password = "your_password"

# ─── 备节点 ─────────────────────────────────────────────
[[cluster.nodes]]
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

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
# password = "your_password"
"#;

// TODO(cluster-rws): 读写分离部署逻辑待实现
const CLUSTER_RWS_PLACEHOLDER: &str = r#"# 达梦数据库读写分离集群配置（占位模板）
# TODO: 读写分离部署逻辑尚未实现
# 架构说明: 基于主备集群，备节点承担只读查询，主节点处理写入
# 使用方式: dm-installer cluster deploy --config dm-cluster-rws.toml

[cluster]
type = "rws"
installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"
oguid = 453331

# ─── 主节点（处理写入） ──────────────────────────────────
[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"
install_path = "/opt/dmdbms"
data_path = "/opt/dmdbms/data"
port = 5236
mal_port = 5237
dw_port = 5238
inst_dw_port = 5239

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

# ─── 备节点（承担只读查询） ──────────────────────────────
[[cluster.nodes]]
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

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;

// TODO(cluster-dsc): DSC 共享存储集群部署逻辑待实现
const CLUSTER_DSC_PLACEHOLDER: &str = r#"# 达梦数据库 DSC 共享存储集群配置（占位模板）
# TODO: DSC 部署逻辑尚未实现
# 架构说明: 多实例共享同一份 SAN/NFS 存储，所有节点访问相同数据文件
# 使用方式: dm-installer cluster deploy --config dm-cluster-dsc.toml

[cluster]
type = "dsc"
installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"
oguid = 453331

# 共享存储路径（SAN 裸设备或 NFS 挂载点，各节点路径必须相同）
shared_storage = "/dev/sdc"

# ─── 节点 1（负责初始化共享实例） ──────────────────────
[[cluster.nodes]]
role = "primary"
host = "192.168.1.10"
instance_name = "DMSVR01"
install_path = "/opt/dmdbms"
data_path = "/dmdata/shared"
port = 5236
mal_port = 5237
dw_port = 5238
inst_dw_port = 5239

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"

# ─── 节点 2 ─────────────────────────────────────────────
[[cluster.nodes]]
role = "standby"
host = "192.168.1.11"
instance_name = "DMSVR02"
install_path = "/opt/dmdbms"
data_path = "/dmdata/shared"
port = 5236
mal_port = 5237
dw_port = 5238
inst_dw_port = 5239

[cluster.nodes.ssh]
user = "root"
identity_file = "~/.ssh/id_rsa"
"#;


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn output_args(path: PathBuf, force: bool) -> InitOutputArgs {
        InitOutputArgs { output: Some(path), force }
    }

    fn cluster_args(kind: ClusterInitKind) -> InitKind {
        InitKind::Cluster(crate::cli::ClusterInitArgs { kind })
    }

    #[test]
    fn test_standalone_creates_file() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("dm-standalone.toml");
        run(&InitKind::Standalone(output_args(out.clone(), false))).unwrap();
        let content = std::fs::read_to_string(&out).unwrap();
        assert!(content.contains("install_path"), "应含 install_path");
        assert!(content.contains("port = 5236"), "应含默认端口");
    }

    #[test]
    fn test_standalone_template_is_valid_toml() {
        let cfg: toml::Value = toml::from_str(STANDALONE_TEMPLATE).expect("单机模板应为合法 TOML");
        assert!(cfg.get("port").is_some(), "应含 port");
    }

    #[test]
    fn test_cluster_ps_creates_file() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("ps.toml");
        let kind = cluster_args(ClusterInitKind::PrimaryStandby(output_args(out.clone(), false)));
        run(&kind).unwrap();
        let content = std::fs::read_to_string(&out).unwrap();
        assert!(content.contains("role = \"primary\""), "应含 primary 节点");
        assert!(content.contains("role = \"standby\""), "应含 standby 节点");
    }

    #[test]
    fn test_cluster_rws_creates_file() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("rws.toml");
        let kind = cluster_args(ClusterInitKind::Rws(output_args(out.clone(), false)));
        run(&kind).unwrap();
        assert!(std::fs::read_to_string(&out).unwrap().contains("read_only = true"));
    }

    #[test]
    fn test_cluster_dsc_creates_file() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("dsc.toml");
        let kind = cluster_args(ClusterInitKind::Dsc(output_args(out.clone(), false)));
        run(&kind).unwrap();
        assert!(std::fs::read_to_string(&out).unwrap().contains("shared_storage"));
    }

    #[test]
    fn test_refuses_to_overwrite_without_force() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("dm-standalone.toml");
        std::fs::write(&out, "existing").unwrap();
        let err = run(&InitKind::Standalone(output_args(out, false))).unwrap_err();
        assert!(format!("{err}").contains("文件已存在"));
    }

    #[test]
    fn test_force_overwrites_existing_file() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("dm-standalone.toml");
        std::fs::write(&out, "old content").unwrap();
        run(&InitKind::Standalone(output_args(out.clone(), true))).unwrap();
        assert!(std::fs::read_to_string(&out).unwrap().contains("install_path"));
    }

    #[test]
    fn test_cluster_ps_template_is_valid_toml() {
        toml::from_str::<toml::Value>(CLUSTER_PS_TEMPLATE).expect("主备模板应为合法 TOML");
    }

    #[test]
    fn test_cluster_rws_placeholder_is_valid_toml() {
        toml::from_str::<toml::Value>(CLUSTER_RWS_PLACEHOLDER).expect("读写分离占位模板应为合法 TOML");
    }

    #[test]
    fn test_cluster_dsc_placeholder_is_valid_toml() {
        toml::from_str::<toml::Value>(CLUSTER_DSC_PLACEHOLDER).expect("DSC 占位模板应为合法 TOML");
    }

}
