//! 节点级安装步骤：环境准备 / 预检 / 上传 / 静默安装 / dminit。

use anyhow::{Context, Result};
use futures::future::try_join_all;

use crate::install::remote_common::{
    check_remote_dmserver_exists, check_remote_prerequisites, upload_and_extract_on_remote,
    run_remote_install,
};
use crate::install::steps::{env_setup, init};

use super::NodeRunner;

pub(super) async fn env_setup_all(pairs: &[NodeRunner<'_>]) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        env_setup::run(*runner)
            .await
            .with_context(|| format!("节点 {} 环境准备失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
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

pub(super) async fn upload_all(pairs: &[NodeRunner<'_>], package_path: &std::path::Path) -> Result<()> {
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

/// 所有节点共用同一对 SYSDBA/SYSAUDITOR 密码（主备守护跨节点连接要求一致凭证）。
pub(super) async fn init_all(
    pairs: &[NodeRunner<'_>],
    sysdba_pwd: &str,
    sysauditor_pwd: &str,
) -> Result<()> {
    let futs = pairs.iter().map(|(node, runner)| async move {
        init::run_dminit(*runner, &node.as_install_config(), sysdba_pwd, sysauditor_pwd)
            .await
            .with_context(|| format!("节点 {} dminit 初始化失败", node.host))
    });
    try_join_all(futs).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::dw::test_support::make_cluster;
    use crate::ssh::{CommandRunner, MockRunner};

    #[tokio::test]
    async fn test_preflight_all_runs_on_every_node() {
        let cluster = make_cluster();
        fn preflight_responses() -> Vec<(String, u32, Vec<u8>)> {
            vec![
                (
                    "grep '^MemTotal:'".to_string(),
                    0,
                    b"MemTotal:       16777216 kB\n".to_vec(),
                ),
                ("nproc".to_string(), 0, b"4\n".to_vec()),
                (
                    "df -B1".to_string(),
                    0,
                    b"Filesystem 1B-blocks Used Available Use% Mounted\n/dev/sda 1 1 99999999999 1% /\n"
                        .to_vec(),
                ),
            ]
        }
        let m1 = MockRunner::new(preflight_responses());
        let m2 = MockRunner::new(preflight_responses());
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&m1, &m2].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();
        preflight_all(&pairs).await.unwrap();
        assert!(!m1.exec_log().is_empty(), "primary 节点应执行预检命令");
        assert!(!m2.exec_log().is_empty(), "standby 节点应执行预检命令");
    }

    #[tokio::test]
    async fn test_upload_all_rejects_existing_dmserver() {
        let cluster = make_cluster();
        let m1 = MockRunner::new(vec![("test -f".to_string(), 0, b"exists\n".to_vec())]);
        let m2 = MockRunner::new(vec![("test -f".to_string(), 0, b"absent\n".to_vec())]);
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&m1, &m2].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();
        let tmp = tempfile::TempDir::new().unwrap();
        let pkg = tmp.path().join("DMInstall.bin");
        std::fs::write(&pkg, b"fake").unwrap();
        let err = upload_all(&pairs, &pkg).await.unwrap_err();
        assert!(
            format!("{err}").contains("已存在达梦数据库"),
            "应报告节点已安装: {err}"
        );
    }

    #[tokio::test]
    async fn test_init_all_uses_shared_password_pair_across_nodes() {
        let cluster = make_cluster();
        let m1 = MockRunner::new(vec![]);
        let m2 = MockRunner::new(vec![]);
        let pairs: Vec<NodeRunner> = cluster
            .nodes
            .iter()
            .zip([&m1, &m2].into_iter().map(|m| m as &dyn CommandRunner))
            .collect();
        init_all(&pairs, "SysdbaPwd1", "AuditorPwd1").await.unwrap();

        for m in [&m1, &m2] {
            let log = m.exec_log();
            assert!(
                log.iter().any(|cmd| cmd.contains("SysdbaPwd1")),
                "每个节点应使用相同的 SYSDBA 密码: {:?}",
                log
            );
            assert!(
                log.iter().any(|cmd| cmd.contains("AuditorPwd1")),
                "每个节点应使用相同的 SYSAUDITOR 密码: {:?}",
                log
            );
        }
    }
}
