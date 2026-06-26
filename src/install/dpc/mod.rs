//! DPC（分布式集群）安装：按达梦 DPC 集群部署文档的步骤顺序编排，
//! 子步骤拆分在各子模块中，本文件负责整体流程与断点续传调度。
//!
//! 与 DW 的关键差异：角色为 SP/BP/MP；无监视器，集群拓扑由 MP + DIsql 系统过程注册建立；
//! 多副本（RAFT）走 dmarch.ini RAFT 段 + dmrman 备份还原 + MOUNT 启动。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::cli::InstallArgs;
use crate::config::dpc::{DpcClusterConfig, DpcNode, DpcRole};
use crate::config::{CommonConfig, InstallerSource};
use crate::install::remote_common::detect_remote_platform;
use crate::install::standalone::{cache_package, generate_password};
use crate::ssh::CommandRunner;

pub mod checkpoint;
pub mod config_files;

mod config_dist;
mod mp_bootstrap;
mod provision;
mod replicate;
mod startup;

#[cfg(test)]
mod test_support;

type NodeRunner<'a> = (&'a DpcNode, &'a dyn CommandRunner);

/// DPC 集群安装入口，对照达梦 DPC 集群部署文档的步骤顺序：
/// 连接预检 → 环境准备 → 上传安装 → dminit → 分发 RAFT 归档（仅多副本） →
/// 启动 MP 并注册 → 副本同步（仅多副本） → 启动 SP/BP。
/// 支持断点续传：已完成的节点级步骤在重跑时会被跳过。
pub async fn run(args: &InstallArgs, common: CommonConfig, cluster: &DpcClusterConfig) -> Result<()> {
    crate::ui::print_banner();

    let multi_replica = cluster.is_multi_replica();
    let total_steps = 8;

    let hosts: Vec<String> = cluster.nodes.iter().map(|n| n.host.clone()).collect();
    let existing_cp = checkpoint::load(cluster.cluster_id)?;
    let mut cp = existing_cp.unwrap_or_else(|| {
        checkpoint::DpcClusterCheckpoint::new(
            cluster.cluster_id,
            generate_password(),
            generate_password(),
            &hosts,
        )
    });
    cp.save()?;
    let sysdba_pwd = cp.sysdba_pwd.clone();
    let sysauditor_pwd = cp.sysauditor_pwd.clone();

    // [1/8] 连接并预检
    crate::ui::step_header(&format!(
        "[1/{total_steps}] 连接并预检 {} 个节点",
        cluster.nodes.len()
    ));
    let sessions = provision::connect_all_nodes(cluster).await?;
    let runners: Vec<&dyn CommandRunner> = sessions.iter().map(|s| s as &dyn CommandRunner).collect();
    let all_pairs: Vec<NodeRunner> = cluster.nodes.iter().zip(runners.iter().copied()).collect();
    provision::preflight_all(&all_pairs).await?;
    crate::ui::log_ok("所有节点预检通过");
    crate::ui::step_footer();

    // 解析安装包（按 MP 节点平台探测，假定集群内节点平台一致）。
    let mp_runner = all_pairs
        .iter()
        .find(|(node, _)| node.role == DpcRole::Mp)
        .map(|(_, runner)| *runner)
        .context("集群中未找到 MP 节点（应已被配置校验拦截）")?;
    let package_path = resolve_cluster_package(args, &common.installer, mp_runner, &mut cp).await?;

    // [2/8] 环境准备
    crate::ui::step_header(&format!("[2/{total_steps}] 环境准备"));
    let pending = pending_pairs(&all_pairs, &cp, |n| n.env_setup_done);
    provision::env_setup_all(&pending).await?;
    for (node, _) in &pending {
        cp.mark(&node.host, |n| n.env_setup_done = true);
    }
    cp.save()?;
    crate::ui::step_footer();

    // [3/8] 上传 + 静默安装
    crate::ui::step_header(&format!("[3/{total_steps}] 上传安装包并静默安装"));
    let pending = pending_pairs(&all_pairs, &cp, |n| n.uploaded);
    provision::upload_all(&pending, &package_path).await?;
    for (node, _) in &pending {
        cp.mark(&node.host, |n| n.uploaded = true);
    }
    cp.save()?;
    let pending = pending_pairs(&all_pairs, &cp, |n| n.installed);
    provision::install_all(&pending).await?;
    for (node, _) in &pending {
        cp.mark(&node.host, |n| n.installed = true);
    }
    cp.save()?;
    crate::ui::log_ok("所有节点安装完成");
    crate::ui::step_footer();

    // [4/8] dminit + 分发 mp.ini
    crate::ui::step_header(&format!("[4/{total_steps}] 初始化数据库实例（dminit）"));
    let pending = pending_pairs(&all_pairs, &cp, |n| n.db_inited);
    provision::dminit_all(&pending, &sysdba_pwd, &sysauditor_pwd).await?;
    config_dist::distribute_mp_ini_all(&pending, cluster).await?;
    for (node, _) in &pending {
        cp.mark(&node.host, |n| n.db_inited = true);
    }
    cp.save()?;
    crate::ui::log_ok("所有节点 dminit 完成，mp.ini 已分发");
    crate::ui::step_footer();

    // [5/8] 分发 RAFT 归档配置（仅多副本）
    crate::ui::step_header(&format!("[5/{total_steps}] 分发 RAFT 归档配置（dmarch.ini）"));
    if multi_replica {
        let pending = pending_pairs(&all_pairs, &cp, |n| n.arch_distributed);
        config_dist::distribute_arch_ini_raft(cluster, &pending).await?;
        for (node, _) in &pending {
            cp.mark(&node.host, |n| n.arch_distributed = true);
        }
        cp.save()?;
        crate::ui::log_ok("BP/MP 节点 dmarch.ini（RAFT 段）已分发");
    } else {
        crate::ui::log_info("单副本模式，跳过 RAFT 归档分发");
    }
    crate::ui::step_footer();

    // [6/8] 启动 MP 并执行集群注册
    crate::ui::step_header(&format!("[6/{total_steps}] 启动 MP 元数据节点并注册集群"));
    let mp_pairs: Vec<NodeRunner> = all_pairs
        .iter()
        .filter(|(n, _)| n.role == DpcRole::Mp)
        .copied()
        .collect();
    let pending_mp = pending_pairs(&mp_pairs, &cp, |n| n.started);
    mp_bootstrap::start_and_register_mp(cluster, &pending_mp, &all_pairs, &sysdba_pwd).await?;
    for (node, _) in &pending_mp {
        cp.mark(&node.host, |n| n.started = true);
    }
    cp.save()?;
    crate::ui::log_ok("MP 节点已启动，集群注册完成");
    crate::ui::step_footer();

    // [7/8] 副本同步（仅多副本）
    crate::ui::step_header(&format!("[7/{total_steps}] 同步非主副本数据"));
    if multi_replica {
        let pending = pending_pairs(&all_pairs, &cp, |n| n.replicated);
        // 同步动作以 raft_group 为单位，pending 仅用于判断是否仍有未完成节点。
        if pending.iter().any(|(n, _)| n.raft_group.is_some()) {
            replicate::replicate_non_primary(cluster, &all_pairs).await?;
        }
        for (node, _) in &pending {
            cp.mark(&node.host, |n| n.replicated = true);
        }
        cp.save()?;
        crate::ui::log_ok("非主副本数据已与主副本同步");
    } else {
        crate::ui::log_info("单副本模式，跳过副本同步");
    }
    crate::ui::step_footer();

    // [8/8] 启动 SP / BP
    crate::ui::step_header(&format!("[8/{total_steps}] 启动 SP / BP 节点"));
    let sp_bp_pairs: Vec<NodeRunner> = all_pairs
        .iter()
        .filter(|(n, _)| matches!(n.role, DpcRole::Sp | DpcRole::Bp))
        .copied()
        .collect();
    let pending = pending_pairs(&sp_bp_pairs, &cp, |n| n.started);
    startup::start_sp_bp_all(cluster, &pending).await?;
    for (node, _) in &pending {
        cp.mark(&node.host, |n| n.started = true);
    }
    cp.save()?;
    crate::ui::log_ok("SP / BP 节点已启动");
    crate::ui::step_footer();

    crate::ui::log_ok(&format!(
        "DPC 集群安装完成。SYSDBA 密码: {}\nSYSAUDITOR 密码: {}\n请妥善保存以上密码（仅本次显示，不会写入磁盘）。",
        sysdba_pwd, sysauditor_pwd
    ));
    checkpoint::DpcClusterCheckpoint::remove(cluster.cluster_id)?;
    Ok(())
}

/// 解析集群安装包路径：CLI --package > checkpoint 缓存 > config.toml installer_package/url
/// > 自动检测下载（按 MP 节点平台，假定集群内节点平台一致）。
async fn resolve_cluster_package(
    args: &InstallArgs,
    installer: &InstallerSource,
    mp_runner: &dyn CommandRunner,
    cp: &mut checkpoint::DpcClusterCheckpoint,
) -> Result<PathBuf> {
    if let Some(p) = &args.package {
        crate::ui::log_info(&format!("使用本地安装包 (CLI --package): {}", p.display()));
        return Ok(p.clone());
    }
    if let Some(cached) = cp
        .package_cache
        .as_ref()
        .map(Path::new)
        .filter(|p| p.exists())
    {
        crate::ui::log_info(&format!("[续] 跳过下载，使用已缓存安装包: {}", cached.display()));
        return Ok(cached.to_path_buf());
    }

    match installer {
        InstallerSource::LocalFile(path) => {
            crate::ui::log_info(&format!("使用本地安装包 (config.toml): {}", path.display()));
            Ok(path.clone())
        }
        InstallerSource::Url(url) => {
            crate::ui::log_info(&format!("下载安装包 (config.toml): {}", url));
            let handle = crate::download::fetch_from_url(url, None).await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
        InstallerSource::Auto => {
            crate::ui::log_info("自动检测 MP 节点平台并下载安装包...");
            let platform = detect_remote_platform(mp_runner).await;
            let handle = crate::download::fetch_dm_installer_for(&platform).await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
    }
}

/// 从节点列表中过滤出某个 checkpoint 标记尚未完成的节点。
fn pending_pairs<'a>(
    all: &[NodeRunner<'a>],
    cp: &checkpoint::DpcClusterCheckpoint,
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
    use test_support::make_single_replica_cluster;

    #[test]
    fn test_pending_pairs_filters_completed_nodes() {
        let cluster = make_single_replica_cluster();
        let mocks: Vec<MockRunner> = cluster.nodes.iter().map(|_| MockRunner::new(vec![])).collect();
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();

        let hosts: Vec<String> = cluster.nodes.iter().map(|n| n.host.clone()).collect();
        let mut cp = checkpoint::DpcClusterCheckpoint::new(
            cluster.cluster_id,
            "p1".into(),
            "p2".into(),
            &hosts,
        );
        cp.mark("192.168.1.10", |n| n.uploaded = true);

        let pending = pending_pairs(&pairs, &cp, |n| n.uploaded);
        assert_eq!(pending.len(), cluster.nodes.len() - 1);
        assert!(pending.iter().all(|(n, _)| n.host != "192.168.1.10"));
    }
}
