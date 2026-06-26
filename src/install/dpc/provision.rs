//! DPC 节点级安装步骤：连接 / 预检 / 环境准备 / 上传 / 静默安装 / dminit。
//! 多为 install::steps + install::remote_common 的薄封装，按 futures::try_join_all 并行。

use anyhow::{Context, Result};
use futures::future::try_join_all;
use std::path::Path;

use crate::config::dpc::{DpcClusterConfig, DpcNode};
use crate::config::ssh::SshCredentials;
use crate::install::remote_common::{
    check_remote_dmserver_exists, check_remote_prerequisites, connect_with_retry, run_remote_install,
    upload_and_extract_on_remote,
};
use crate::install::steps::env_setup;
use crate::ssh::{CommandRunner, SshSession, shell_quote};

use super::NodeRunner;

/// DPC 节点 SSH 默认端口/重试参数，dpc.toml 暂不开放配置（与 dw 一致，保持配置最小化）。
const DEFAULT_SSH_PORT: u16 = 22;
const DEFAULT_MAX_RETRIES: u32 = 3;
const DEFAULT_RETRY_INTERVAL_SECS: u64 = 5;

pub(super) async fn connect_all_nodes(cluster: &DpcClusterConfig) -> Result<Vec<SshSession>> {
    let futs = cluster.nodes.iter().map(|node| async move {
        let creds: SshCredentials = node.ssh.clone();
        connect_with_retry(
            &node.host,
            DEFAULT_SSH_PORT,
            &creds,
            DEFAULT_MAX_RETRIES,
            DEFAULT_RETRY_INTERVAL_SECS,
        )
        .await
        .with_context(|| format!("连接节点 {} 失败", node.host))
    });
    try_join_all(futs).await
}

pub(super) async fn preflight_all(pairs: &[NodeRunner<'_>]) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        check_remote_prerequisites(&node.as_install_config(), *runner, false)
            .await
            .with_context(|| format!("节点 {} 预检失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
}

pub(super) async fn env_setup_all(pairs: &[NodeRunner<'_>]) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        env_setup::run(*runner)
            .await
            .with_context(|| format!("节点 {} 环境准备失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
}

pub(super) async fn upload_all(pairs: &[NodeRunner<'_>], package_path: &Path) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        if check_remote_dmserver_exists(&node.as_install_config(), *runner).await? {
            anyhow::bail!(
                "节点 {} 的安装目录 {} 已存在达梦数据库（dmserver），\n\
                 请先卸载或在配置文件中修改 install_path",
                node.host,
                node.install_path
            );
        }
        upload_and_extract_on_remote(package_path, *runner)
            .await
            .with_context(|| format!("节点 {} 上传安装包失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
}

pub(super) async fn install_all(pairs: &[NodeRunner<'_>]) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        run_remote_install(&node.as_install_config(), *runner)
            .await
            .with_context(|| format!("节点 {} 静默安装失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
}

/// 所有节点共用同一对 SYSDBA/SYSAUDITOR 密码（集群跨节点连接要求一致凭证）。
///
/// 选型说明：steps::init::run_dminit 是固定参数的薄封装，不接受额外的
/// dpc_mode= / ap_port_num= 参数（这两个参数是 DPC dminit 特有的）。为不破坏
/// standalone/dw 既有调用方，这里直接在本模块按 init.rs 的 su-dmdba 模式拼接
/// dminit 命令，并追加 `dpc_mode=<SP|BP|MP>` 与 `ap_port_num=<N>`。
pub(super) async fn dminit_all(
    pairs: &[NodeRunner<'_>],
    sysdba_pwd: &str,
    sysauditor_pwd: &str,
) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        run_dpc_dminit(*runner, node, sysdba_pwd, sysauditor_pwd)
            .await
            .with_context(|| format!("节点 {} dminit 初始化失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
}

/// 以 dmdba 用户身份执行 DPC dminit，追加 dpc_mode= / ap_port_num= 参数。
async fn run_dpc_dminit(
    runner: &dyn CommandRunner,
    node: &DpcNode,
    sysdba_pwd: &str,
    sysauditor_pwd: &str,
) -> Result<()> {
    let dminit = format!("{}/bin/dminit", node.install_path);
    let inner_cmd = format!(
        "{} PATH={} DB_NAME=DAMENG INSTANCE_NAME={} PORT_NUM={} PAGE_SIZE={} EXTENT_SIZE={} CHARSET={} CASE_SENSITIVE={} SYSDBA_PWD={} SYSAUDITOR_PWD={} dpc_mode={} ap_port_num={}",
        shell_quote(&dminit),
        shell_quote(&node.data_path),
        shell_quote(&node.instance_name),
        node.port,
        node.page_size,
        node.extent_size,
        node.charset,
        if node.case_sensitive { "Y" } else { "N" },
        shell_quote(sysdba_pwd),
        shell_quote(sysauditor_pwd),
        node.role.as_str(),
        node.ap_port,
    );
    let cmd = format!("su - dmdba -c {}", shell_quote(&inner_cmd));
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("dminit 执行失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dpc::DpcRole;
    use crate::install::dpc::test_support::make_single_replica_cluster;
    use crate::ssh::MockRunner;

    #[tokio::test]
    async fn test_dminit_all_appends_dpc_mode_and_ap_port() {
        let cluster = make_single_replica_cluster();
        let mocks: Vec<MockRunner> = cluster.nodes.iter().map(|_| MockRunner::new(vec![])).collect();
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();
        dminit_all(&pairs, "SysdbaPwd1", "AuditorPwd1").await.unwrap();

        // MP 节点应带 dpc_mode=MP
        let mp_idx = cluster.nodes.iter().position(|n| n.role == DpcRole::Mp).unwrap();
        let mp_log = mocks[mp_idx].exec_log();
        assert!(
            mp_log.iter().any(|c| c.contains("dpc_mode=MP") && c.contains("ap_port_num=5237")),
            "MP 节点 dminit 应含 dpc_mode=MP ap_port_num=5237: {:?}",
            mp_log
        );
        // 每个节点都应使用相同密码且以 dmdba 身份执行
        for m in &mocks {
            let log = m.exec_log();
            assert!(log.iter().any(|c| c.starts_with("su - dmdba -c")));
            assert!(log.iter().any(|c| c.contains("SysdbaPwd1")));
        }
    }

    #[tokio::test]
    async fn test_upload_all_rejects_existing_dmserver() {
        let cluster = make_single_replica_cluster();
        let mocks: Vec<MockRunner> = cluster
            .nodes
            .iter()
            .map(|_| MockRunner::new(vec![("test -f".to_string(), 0, b"exists\n".to_vec())]))
            .collect();
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip(mocks.iter().map(|m| m as &dyn CommandRunner))
            .collect();
        let tmp = tempfile::TempDir::new().unwrap();
        let pkg = tmp.path().join("DMInstall.bin");
        std::fs::write(&pkg, b"fake").unwrap();
        let err = upload_all(&pairs, &pkg).await.unwrap_err();
        assert!(format!("{err}").contains("已存在达梦数据库"), "实际: {err}");
    }
}
