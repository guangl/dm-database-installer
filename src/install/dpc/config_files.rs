//! DPC 集群配置文件生成：mp.ini（元数据节点地址）与多副本 dmarch.ini（RAFT 归档段）。
//! 字段含义参考达梦 DPC 集群部署文档（mp.ini / dmarch.ini RAFT 归档）。

use crate::config::dpc::{DpcClusterConfig, DpcNode};

// RAFT 全局参数默认值，参照达梦 DPC 多副本归档常用配置（与 dw.rs 的 MAL/心跳默认值同量级）。
const RAFT_XMAL_HB_INTERVAL: u32 = 6000;
const RAFT_HB_INTERVAL: u32 = 3000;
const RAFT_VOTE_INTERVAL: u32 = 10000;

/// mp.ini：所有节点共用，指向 MP 元数据节点的地址与端口。
pub fn mp_ini(cluster: &DpcClusterConfig) -> String {
    format!(
        "mp_host = {mp_host}\nmp_port = {mp_port}\n",
        mp_host = cluster.mp_host,
        mp_port = cluster.mp_port,
    )
}

/// dmarch.ini（多副本专用）：RAFT 全局参数 + 每个对端副本一个 [ARCHIVE_RAFT{n}] 段 + 本地归档段。
/// `peers` 为同一 raft_group 内除本节点外的其他副本（用于生成 RAFT 归档目标）。
pub fn dmarch_ini_raft(node: &DpcNode, peers: &[&DpcNode], _cluster: &DpcClusterConfig) -> String {
    let self_id = node.raft_self_id.unwrap_or(0);
    let mut out = format!(
        "XMAL_HB_INTERVAL = {xmal_hb}\n\
         RAFT_HB_INTERVAL = {raft_hb}\n\
         RAFT_VOTE_INTERVAL = {raft_vote}\n\
         RAFT_SELF_ID = {self_id}\n\n",
        xmal_hb = RAFT_XMAL_HB_INTERVAL,
        raft_hb = RAFT_HB_INTERVAL,
        raft_vote = RAFT_VOTE_INTERVAL,
    );
    for (idx, peer) in peers.iter().enumerate() {
        out.push_str(&format!(
            "[ARCHIVE_RAFT{n}]\n\
             ARCH_TYPE = RAFT\n\
             ARCH_DEST = {dest}\n\
             ARCH_DEST_ID = {dest_id}\n\n",
            n = idx + 1,
            dest = peer.instance_name,
            dest_id = peer.raft_self_id.unwrap_or(0),
        ));
    }
    out.push_str(&format!(
        "[ARCHIVE_LOCAL1]\n\
         ARCH_TYPE = LOCAL\n\
         ARCH_DEST = {data_path}/arch\n\
         ARCH_FILE_SIZE = 128\n\
         ARCH_SPACE_LIMIT = 0\n",
        data_path = node.data_path,
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::dpc::test_support::make_multi_replica_cluster;

    #[test]
    fn test_mp_ini_renders_host_and_port() {
        let cluster = make_multi_replica_cluster();
        let ini = mp_ini(&cluster);
        assert!(ini.contains("mp_host = 192.168.1.10"), "实际: {ini}");
        assert!(ini.contains("mp_port = 5238"), "实际: {ini}");
    }

    #[test]
    fn test_dmarch_ini_raft_renders_global_and_peer_sections() {
        let cluster = make_multi_replica_cluster();
        let members = cluster.raft_group_members("RAFT1");
        let primary = members[0]; // self_id = 1
        let peers: Vec<&DpcNode> = members.iter().copied().filter(|n| n.host != primary.host).collect();
        let ini = dmarch_ini_raft(primary, &peers, &cluster);

        assert!(ini.contains("RAFT_SELF_ID = 1"), "实际: {ini}");
        assert!(ini.contains("RAFT_HB_INTERVAL ="), "实际: {ini}");
        assert!(ini.contains("[ARCHIVE_RAFT1]"), "实际: {ini}");
        assert!(ini.contains("ARCH_TYPE = RAFT"), "实际: {ini}");
        assert!(ini.contains("ARCH_DEST = BP02"), "实际: {ini}");
        assert!(ini.contains("ARCH_DEST_ID = 2"), "实际: {ini}");
        // 本地归档段
        assert!(ini.contains("[ARCHIVE_LOCAL1]"), "实际: {ini}");
        assert!(ini.contains("ARCH_TYPE = LOCAL"), "实际: {ini}");
        assert!(ini.contains("/opt/dmdbms/data/arch"), "实际: {ini}");
        assert!(ini.contains("ARCH_FILE_SIZE = 128"), "实际: {ini}");
    }
}
