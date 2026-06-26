//! DPC 特有的核心步骤：启动 MP 元数据节点并通过 DIsql 执行集群注册。
//! 无 DW 对应物——DPC 不使用监视器，集群拓扑由 MP 节点 + 系统过程注册建立。
//! 参见达梦 DPC 集群部署文档（SP_CREATE_DPC_INSTANCE / SP_CREATE_DPC_RAFT / SP_CREATE_DPC_BP_GROUP）。

use anyhow::{Context, Result};

use crate::config::dpc::{DpcClusterConfig, DpcNode, DpcRole};
use crate::install::steps::service;
use crate::ssh::{CommandRunner, shell_quote};

use super::NodeRunner;

const REGISTER_SQL_PATH: &str = "/tmp/dm_dpc_register.sql";

/// 启动 MP 节点并完成集群注册。
/// `mp_pairs` 为待处理的 MP 节点（断点续传后可能只剩部分）；`all_pairs` 为全量节点，
/// 用于按 mp_host 定位执行注册的运行器。
pub(super) async fn start_and_register_mp(
    cluster: &DpcClusterConfig,
    mp_pairs: &[NodeRunner<'_>],
    all_pairs: &[NodeRunner<'_>],
    sysdba_pwd: &str,
) -> Result<()> {
    // 启动所有待处理的 MP 节点（dpc_mode=MP，后台常驻）。
    for (node, runner) in mp_pairs {
        start_mp_server(*runner, node)
            .await
            .with_context(|| format!("MP 节点 {} 启动失败", node.host))?;
        service::wait_process_alive(*runner, 60)
            .await
            .with_context(|| format!("MP 节点 {} dmserver 未在预期时间内启动", node.host))?;
    }

    // 在 mp_host 对应的节点上执行 DIsql 注册（注册是一次性集群级动作）。
    let (mp_node, mp_runner) = all_pairs
        .iter()
        .find(|(n, _)| n.role == DpcRole::Mp && n.host == cluster.mp_host)
        .or_else(|| all_pairs.iter().find(|(n, _)| n.role == DpcRole::Mp))
        .copied()
        .context("集群中未找到 MP 节点（应已被配置校验拦截）")?;

    let sql = build_register_sql(cluster);
    mp_runner
        .sftp_write(REGISTER_SQL_PATH, sql.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("写入 DPC 注册脚本失败: {e}"))?;

    let disql = format!("{}/bin/disql", mp_node.install_path);
    let conn = format!("SYSDBA/{}@{}:{}", sysdba_pwd, cluster.mp_host, cluster.mp_port);
    let inner = format!(
        "{} {} -e {}",
        shell_quote(&disql),
        shell_quote(&conn),
        shell_quote(REGISTER_SQL_PATH),
    );
    let cmd = format!("su - dmdba -c {}", shell_quote(&inner));
    mp_runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("执行 DPC 集群注册失败: {e}"))?;

    // 尽力清理临时脚本，失败不影响整体结果。
    let _ = mp_runner
        .exec(&format!("rm -f {}", shell_quote(REGISTER_SQL_PATH)))
        .await;
    Ok(())
}

/// 以 dmdba 身份后台启动 dmserver（dpc_mode=MP），输出重定向到日志文件。
/// DPC 元数据节点需先于其他角色常驻；这里沿用其他模块对 dm 进程的 su-dmdba 启动风格，
/// 以 nohup 后台化（DPC 注册需要 MP 在线，但安装编排不阻塞在该前台进程上）。
async fn start_mp_server(runner: &dyn CommandRunner, node: &DpcNode) -> Result<()> {
    let dm_ini = service::dm_ini_path(&node.as_install_config());
    let dmserver = format!("{}/bin/dmserver", node.install_path);
    let log = format!("{}/DAMENG/dmserver_dpc.log", node.data_path);
    let inner = format!(
        "nohup {} {} dpc_mode=MP >{} 2>&1 &",
        shell_quote(&dmserver),
        shell_quote(&dm_ini),
        shell_quote(&log),
    );
    let cmd = format!("su - dmdba -c {}", shell_quote(&inner));
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("启动 MP dmserver 失败: {e}"))?;
    Ok(())
}

/// 构造 DPC 集群注册 SQL（作为 disql 脚本一次性执行）。
fn build_register_sql(cluster: &DpcClusterConfig) -> String {
    let mut sql = String::new();

    // 1. 注册 MP 节点自身。
    for mp in cluster.nodes_with_role(DpcRole::Mp) {
        sql.push_str(&format!(
            "SP_CREATE_DPC_INSTANCE('{name}', '{host}', {ap_port}, 'MP', 'NORMAL', 0);\n",
            name = mp.instance_name,
            host = mp.host,
            ap_port = mp.ap_port,
        ));
    }

    if cluster.is_multi_replica() {
        // 2. 多副本：每个 raft_group 先建 RAFT，再注册组内各副本实例。
        //    主副本（raft_self_id==1）以 NORMAL + disk_size=1 注册，其余为 STANDBY + 0。
        for group in cluster.raft_groups() {
            sql.push_str(&format!("SP_CREATE_DPC_RAFT('{group}');\n"));
            for member in cluster.raft_group_members(&group) {
                let is_primary = member.raft_self_id == Some(1);
                let (status, disk_size) = if is_primary { ("NORMAL", 1) } else { ("STANDBY", 0) };
                sql.push_str(&format!(
                    "SP_CREATE_DPC_INSTANCE('{name}', '{host}', {ap_port}, '{role}', '{status}', {disk_size}, '{group}');\n",
                    name = member.instance_name,
                    host = member.host,
                    ap_port = member.ap_port,
                    role = member.role.as_str(),
                ));
            }
        }
        // 3. BP_GROUP 聚合（仅多副本）。
        for bg in &cluster.bp_groups {
            sql.push_str(&format!("SP_CREATE_DPC_BP_GROUP('{}');\n", bg.name));
            for raft in &bg.rafts {
                sql.push_str(&format!(
                    "SP_BP_GROUP_ADD_RAFT('{group}', '{raft}');\n",
                    group = bg.name,
                ));
            }
        }
    } else {
        // 2'. 单副本：直接以 NORMAL 注册各 BP/SP 实例（无 RAFT 组）。
        for node in &cluster.nodes {
            if node.role == DpcRole::Mp {
                continue;
            }
            sql.push_str(&format!(
                "SP_CREATE_DPC_INSTANCE('{name}', '{host}', {ap_port}, '{role}', 'NORMAL', 0);\n",
                name = node.instance_name,
                host = node.host,
                ap_port = node.ap_port,
                role = node.role.as_str(),
            ));
        }
    }

    sql.push_str("exit;\n");
    sql
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::dpc::test_support::{make_multi_replica_cluster, make_single_replica_cluster};
    use crate::ssh::MockRunner;

    #[test]
    fn test_build_register_sql_single_replica() {
        let cluster = make_single_replica_cluster();
        let sql = build_register_sql(&cluster);
        // MP 自身 + 各 BP/SP NORMAL 注册
        assert!(sql.contains("SP_CREATE_DPC_INSTANCE('MP01'"), "实际: {sql}");
        assert!(sql.contains("'MP', 'NORMAL'"), "实际: {sql}");
        assert!(sql.contains("SP_CREATE_DPC_INSTANCE('BP01'"), "实际: {sql}");
        assert!(sql.contains("SP_CREATE_DPC_INSTANCE('SP01'"), "实际: {sql}");
        // 单副本不应出现 RAFT / BP_GROUP
        assert!(!sql.contains("SP_CREATE_DPC_RAFT"), "实际: {sql}");
        assert!(!sql.contains("SP_CREATE_DPC_BP_GROUP"), "实际: {sql}");
        assert!(sql.trim_end().ends_with("exit;"), "实际: {sql}");
    }

    #[test]
    fn test_build_register_sql_multi_replica() {
        let cluster = make_multi_replica_cluster();
        let sql = build_register_sql(&cluster);
        assert!(sql.contains("SP_CREATE_DPC_RAFT('RAFT1')"), "实际: {sql}");
        // 主副本 NORMAL + disk_size 1，备副本 STANDBY + 0
        assert!(sql.contains("SP_CREATE_DPC_INSTANCE('BP01'") && sql.contains("'NORMAL', 1, 'RAFT1'"), "实际: {sql}");
        assert!(sql.contains("SP_CREATE_DPC_INSTANCE('BP02'") && sql.contains("'STANDBY', 0, 'RAFT1'"), "实际: {sql}");
        // BP_GROUP 聚合
        assert!(sql.contains("SP_CREATE_DPC_BP_GROUP('BG1')"), "实际: {sql}");
        assert!(sql.contains("SP_BP_GROUP_ADD_RAFT('BG1', 'RAFT1')"), "实际: {sql}");
    }

    #[tokio::test]
    async fn test_start_and_register_mp_writes_sql_and_runs_disql() {
        let cluster = make_single_replica_cluster();
        // 进程存活探测返回 alive
        let mocks: Vec<MockRunner> = cluster
            .nodes
            .iter()
            .map(|_| MockRunner::new(vec![("pgrep".to_string(), 0, b"alive\n".to_vec())]))
            .collect();
        let all_pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();
        let mp_pairs: Vec<NodeRunner> = all_pairs
            .iter()
            .filter(|(n, _)| n.role == DpcRole::Mp)
            .copied()
            .collect();

        start_and_register_mp(&cluster, &mp_pairs, &all_pairs, "Pwd123")
            .await
            .unwrap();

        // mp_host 节点应写注册脚本并执行 disql -e
        let mp_idx = cluster.nodes.iter().position(|n| n.host == cluster.mp_host).unwrap();
        let sftp = mocks[mp_idx].sftp_log();
        assert!(sftp.iter().any(|(p, _)| p == REGISTER_SQL_PATH), "应写注册脚本");
        let log = mocks[mp_idx].exec_log();
        assert!(
            log.iter().any(|c| c.contains("disql") && c.contains("-e") && c.contains(REGISTER_SQL_PATH)),
            "应执行 disql 注册脚本: {:?}",
            log
        );
        assert!(
            log.iter().any(|c| c.contains("dpc_mode=MP")),
            "应以 dpc_mode=MP 启动: {:?}",
            log
        );
    }
}
