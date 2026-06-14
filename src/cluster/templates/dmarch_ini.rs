use crate::config::{ArchiveConfig, format_local_arch_section, resolve_arch_path};
use crate::config::cluster::{DminitConfig, NodeConfig};

/// 生成 dmarch.ini 内容（主备节点 ARCH_DEST 方向相反）。
///
/// - 主节点：ARCH_DEST = 备节点实例名（实时归档目标为备节点）
/// - 备节点：ARCH_DEST = 主节点实例名（用于角色切换时）
pub fn generate_dmarch_ini(
    _node: &NodeConfig,
    dminit: &DminitConfig,
    peer_instance: &str,
    archive: &ArchiveConfig,
) -> String {
    let local_arch = resolve_arch_path(archive, &dminit.data_path);
    format!(
        "[ARCHIVE_REALTIME]\nARCH_TYPE = REALTIME\nARCH_DEST = {}\n\n{}",
        peer_instance,
        format_local_arch_section(&local_arch, archive),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::cluster::{DminitConfig, ArchiveConfig, NodeConfig, NodeRole, SshCredentials};
    use std::path::PathBuf;

    fn make_dminit() -> DminitConfig {
        DminitConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            port: 5236,
            page_size: 8,
            charset: 0,
            case_sensitive: true,
            extent_size: 16,
        }
    }

    fn make_primary() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Primary,
            host: "192.168.1.10".to_string(),
            instance_name: "DMSVR01".to_string(),
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            read_only: false,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: Some(PathBuf::from("~/.ssh/id_rsa")),
                password: None,
            },
        }
    }

    fn make_standby() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Standby,
            host: "192.168.1.11".to_string(),
            instance_name: "DMSVR02".to_string(),
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            read_only: false,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: Some(PathBuf::from("~/.ssh/id_rsa")),
                password: None,
            },
        }
    }

    #[test]
    fn test_dmarch_ini_primary_dest_is_standby() {
        let primary = make_primary();
        let dminit = make_dminit();
        let archive = ArchiveConfig::default();
        let ini = generate_dmarch_ini(&primary, &dminit, "DMSVR02", &archive);
        assert!(ini.contains("ARCH_DEST = DMSVR02"), "主节点 ARCH_DEST 应指向备节点 DMSVR02，实际: {ini}");
        assert!(ini.contains("ARCH_TYPE = REALTIME"), "应含 REALTIME 段");
        assert!(ini.contains("ARCH_TYPE = LOCAL"), "应含 LOCAL 段");
        assert!(ini.contains("ARCH_DEST = /opt/dmdbms/data/arch"), "应含本地归档路径");
    }

    #[test]
    fn test_dmarch_ini_standby_dest_is_primary() {
        let dminit = make_dminit();
        let archive = ArchiveConfig::default();
        let standby = make_standby();
        let ini = generate_dmarch_ini(&standby, &dminit, "DMSVR01", &archive);
        assert!(ini.contains("ARCH_DEST = DMSVR01"), "备节点 ARCH_DEST 应指向主节点 DMSVR01，实际: {ini}");
        let primary = make_primary();
        let primary_ini = generate_dmarch_ini(&primary, &dminit, "DMSVR02", &archive);
        let primary_dest = "ARCH_DEST = DMSVR02";
        let standby_dest = "ARCH_DEST = DMSVR01";
        assert!(primary_ini.contains(primary_dest), "主节点必须含 {primary_dest}");
        assert!(ini.contains(standby_dest), "备节点必须含 {standby_dest}");
        assert_ne!(primary_dest, standby_dest, "主备 ARCH_DEST 必须不同");
    }

    #[test]
    fn test_dmarch_ini_custom_arch_path() {
        let node = make_primary();
        let dminit = make_dminit();
        let archive = ArchiveConfig {
            arch_path: Some("/data/archive".to_string()),
            file_size: 256,
            space_limit: 10240,
            hang_flag: true,
            compressed: false,
        };
        let ini = generate_dmarch_ini(&node, &dminit, "DMSVR02", &archive);
        assert!(ini.contains("ARCH_DEST = /data/archive"), "应使用自定义归档路径");
        assert!(ini.contains("ARCH_FILE_SIZE = 256"), "应使用自定义 file_size");
        assert!(ini.contains("ARCH_SPACE_LIMIT = 10240"), "应使用自定义 space_limit");
    }

    #[test]
    fn test_dmarch_ini_hang_flag_and_compressed() {
        let node = make_primary();
        let dminit = make_dminit();
        let archive = ArchiveConfig { hang_flag: false, compressed: true, ..ArchiveConfig::default() };
        let ini = generate_dmarch_ini(&node, &dminit, "DMSVR02", &archive);
        assert!(ini.contains("ARCH_HANG_FLAG = 0"), "hang_flag=false 应输出 0");
        assert!(ini.contains("ARCH_COMPRESSED = 1"), "compressed=true 应输出 1");
    }
}
