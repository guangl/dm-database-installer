//! 主备集群（DW）安装：按达梦官方数据守护搭建文档的步骤顺序编排，
//! 具体步骤拆分在各子模块中，本文件只负责整体流程与断点续传调度。

use anyhow::Result;

use crate::cli::InstallArgs;
use crate::config::dw::{DwNode, NodeRole};
use crate::config::{CommonConfig, DwClusterConfig};
use crate::install::standalone::generate_password;
use crate::ssh::CommandRunner;

pub mod checkpoint;
pub mod config_files;

mod config_dist;
mod connect;
mod post_setup;
mod provision;
mod startup;
mod sync;

#[cfg(test)]
mod test_support;

type NodeRunner<'a> = (&'a DwNode, &'a dyn CommandRunner);

/// 主备集群安装入口，对照达梦官方数据守护搭建文档的步骤顺序：
/// 连接预检 → 环境准备 → 上传 → 静默安装 → dminit → 备份还原同步备库 → 分发守护配置
/// → mount 模式启动 + 设置 OGUID/角色 → 启动 dmwatcher/dmmonitor → 备份作业/SQL日志/参数优化。
/// 支持断点续传：已完成的节点级步骤在重跑时会被跳过。
pub async fn run(args: &InstallArgs, common: CommonConfig, cluster: &DwClusterConfig) -> Result<()> {
    crate::ui::print_banner();

    let hosts: Vec<String> = cluster.nodes.iter().map(|n| n.host.clone()).collect();
    let existing_cp = checkpoint::load(cluster.oguid)?;
    let mut cp = existing_cp.unwrap_or_else(|| {
        checkpoint::ClusterCheckpoint::new(
            cluster.oguid,
            generate_password(),
            generate_password(),
            &hosts,
        )
    });
    cp.save()?;
    let sysdba_pwd = cp.sysdba_pwd.clone();
    let sysauditor_pwd = cp.sysauditor_pwd.clone();

    crate::ui::step_header(&format!("[1/10] 连接并预检 {} 个节点", cluster.nodes.len()));
    let sessions = connect::connect_all_nodes(cluster).await?;
    let runners: Vec<&dyn CommandRunner> =
        sessions.iter().map(|s| s as &dyn CommandRunner).collect();
    let all_pairs: Vec<NodeRunner> = cluster.nodes.iter().zip(runners.iter().copied()).collect();
    provision::preflight_all(&all_pairs).await?;
    crate::ui::log_ok("所有节点预检通过");
    crate::ui::step_footer();

    let primary_runner = all_pairs
        .iter()
        .find(|(node, _)| node.host == cluster.primary().host)
        .map(|(_, runner)| *runner)
        .expect("primary 节点必然在 all_pairs 中");
    let package_path =
        connect::resolve_cluster_package(args, &common.installer, primary_runner, &mut cp).await?;

    crate::ui::step_header("[2/10] 环境准备");
    let pending = pending_pairs(&all_pairs, &cp, |n| n.env_setup_done);
    provision::env_setup_all(&pending).await?;
    mark_all_and_save(&mut cp, &pending, |n| n.env_setup_done = true)?;
    crate::ui::step_footer();

    crate::ui::step_header("[3/10] 上传安装包");
    let pending = pending_pairs(&all_pairs, &cp, |n| n.uploaded);
    provision::upload_all(&pending, &package_path).await?;
    mark_all_and_save(&mut cp, &pending, |n| n.uploaded = true)?;
    crate::ui::step_footer();

    crate::ui::step_header("[4/10] 静默安装");
    let pending = pending_pairs(&all_pairs, &cp, |n| n.installed);
    provision::install_all(&pending).await?;
    mark_all_and_save(&mut cp, &pending, |n| n.installed = true)?;
    crate::ui::log_ok("所有节点安装完成");
    crate::ui::step_footer();

    crate::ui::step_header("[5/10] 初始化数据库实例");
    let pending = pending_pairs(&all_pairs, &cp, |n| n.db_inited);
    provision::init_all(&pending, &sysdba_pwd, &sysauditor_pwd).await?;
    mark_all_and_save(&mut cp, &pending, |n| n.db_inited = true)?;
    crate::ui::log_ok("所有节点 dminit 完成");
    crate::ui::step_footer();

    crate::ui::step_header("[6/10] 备份还原同步备库数据");
    let pending = pending_pairs(&all_pairs, &cp, |n| n.synced);
    let pending_standbys: Vec<NodeRunner> = pending
        .into_iter()
        .filter(|(node, _)| node.role == NodeRole::Standby)
        .collect();
    sync::sync_standbys_from_primary(cluster, &all_pairs, &pending_standbys).await?;
    mark_all_and_save(&mut cp, &pending_standbys, |n| n.synced = true)?;
    crate::ui::log_ok("备库数据已与主库同步");
    crate::ui::step_footer();

    crate::ui::step_header("[7/10] 分发主备守护配置");
    let pending = pending_pairs(&all_pairs, &cp, |n| n.config_distributed);
    config_dist::distribute_config_all(cluster, &pending).await?;
    mark_all_and_save(&mut cp, &pending, |n| n.config_distributed = true)?;
    crate::ui::log_ok("dmmal.ini / dmarch.ini / dmwatcher.ini 已分发，dm.ini 已更新");
    crate::ui::step_footer();

    crate::ui::step_header("[8/10] 启动数据库（mount 模式）并设置 OGUID / 角色");
    let pending = pending_pairs(&all_pairs, &cp, |n| n.mount_started);
    startup::start_databases(cluster, &pending, &sysdba_pwd).await?;
    mark_all_and_save(&mut cp, &pending, |n| n.mount_started = true)?;
    crate::ui::log_ok("primary/standby 均已 mount 启动并完成角色设置");
    crate::ui::step_footer();

    crate::ui::step_header("[9/10] 启动数据守护进程 dmwatcher 与监视器 dmmonitor");
    let pending = pending_pairs(&all_pairs, &cp, |n| n.watcher_started);
    startup::start_watchers_all(&pending).await?;
    mark_all_and_save(&mut cp, &pending, |n| n.watcher_started = true)?;
    let monitor_node = cluster.monitor_node();
    if !cp.node(&monitor_node.host).monitor_started {
        if let Some((_, monitor_runner)) = all_pairs.iter().find(|(n, _)| n.host == monitor_node.host) {
            startup::start_monitor(cluster, monitor_node, *monitor_runner).await?;
            cp.mark(&monitor_node.host, |n| n.monitor_started = true);
            cp.save()?;
        }
    } else {
        crate::ui::log_info("[续] dmmonitor 已启动，跳过");
    }
    crate::ui::log_ok("dmwatcher 与 dmmonitor 已启动");
    crate::ui::step_footer();

    crate::ui::step_header("[10/10] 配置备份作业 / 开启 SQL 日志 / 应用参数优化");
    let pending = pending_pairs(&all_pairs, &cp, |n| n.backup_configured);
    post_setup::backup_all(&pending, &sysdba_pwd).await?;
    mark_all_and_save(&mut cp, &pending, |n| n.backup_configured = true)?;

    let pending = pending_pairs(&all_pairs, &cp, |n| n.sql_log_enabled);
    post_setup::sql_log_all(&pending, &sysdba_pwd).await?;
    mark_all_and_save(&mut cp, &pending, |n| n.sql_log_enabled = true)?;

    let pending = pending_pairs(&all_pairs, &cp, |n| n.param_tuned);
    post_setup::param_tune_all(&pending, &sysdba_pwd).await?;
    mark_all_and_save(&mut cp, &pending, |n| n.param_tuned = true)?;
    crate::ui::log_ok("备份作业 / SQL 日志 / 参数优化已完成");
    crate::ui::step_footer();

    crate::ui::log_ok(&format!(
        "主备集群安装完成。SYSDBA 密码: {}\nSYSAUDITOR 密码: {}\n请妥善保存以上密码（仅本次显示，不会写入磁盘）。",
        sysdba_pwd, sysauditor_pwd
    ));
    checkpoint::ClusterCheckpoint::remove(cluster.oguid)?;
    Ok(())
}

/// 将 `pending` 中每个节点标记为已完成当前步骤并持久化 checkpoint。
/// 各节点步骤共用的"标记 + 保存"收尾动作，避免在每个步骤里重复写 for 循环。
fn mark_all_and_save(
    cp: &mut checkpoint::ClusterCheckpoint,
    pending: &[NodeRunner],
    mark: impl Fn(&mut checkpoint::NodeCheckpoint),
) -> Result<()> {
    for (node, _) in pending {
        cp.mark(&node.host, &mark);
    }
    cp.save()
}

/// 从全量节点列表中过滤出某个 checkpoint 标记尚未完成的节点。
fn pending_pairs<'a>(
    all: &[NodeRunner<'a>],
    cp: &checkpoint::ClusterCheckpoint,
    done: impl Fn(&checkpoint::NodeCheckpoint) -> bool,
) -> Vec<NodeRunner<'a>> {
    let pending: Vec<NodeRunner<'a>> = all
        .iter()
        .filter(|(node, _)| !done(&cp.node(&node.host)))
        .copied()
        .collect();
    let skipped = all.len() - pending.len();
    if skipped > 0 {
        crate::ui::log_info(&format!("[续] {} 个节点已完成此步骤，跳过", skipped));
    }
    pending
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockRunner;
    use test_support::make_cluster;

    #[test]
    fn test_pending_pairs_filters_completed_nodes() {
        let cluster = make_cluster();
        let m1 = MockRunner::new(vec![]);
        let m2 = MockRunner::new(vec![]);
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&m1, &m2].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();

        let mut cp = checkpoint::ClusterCheckpoint::new(
            453331,
            "p1".into(),
            "p2".into(),
            &["192.168.1.10".to_string(), "192.168.1.11".to_string()],
        );
        cp.mark("192.168.1.10", |n| n.uploaded = true);

        let pending = pending_pairs(&pairs, &cp, |n| n.uploaded);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0.host, "192.168.1.11");
    }
}
