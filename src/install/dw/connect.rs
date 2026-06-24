//! SSH 连接与安装包来源解析。

use anyhow::{Context, Result};
use futures::future::try_join_all;
use std::path::{Path, PathBuf};

use crate::cli::InstallArgs;
use crate::config::dw::DwNode;
use crate::config::ssh::SshCredentials;
use crate::config::{DwClusterConfig, InstallerSource};
use crate::install::remote_common::{connect_with_retry, detect_remote_platform};
use crate::install::standalone::cache_package;
use crate::ssh::{CommandRunner, SshSession};

use super::checkpoint::ClusterCheckpoint;

/// 集群节点 SSH 默认端口/重试参数，dw.toml 暂不开放配置（保持配置最小化）。
const DEFAULT_SSH_PORT: u16 = 22;
const DEFAULT_MAX_RETRIES: u32 = 3;
const DEFAULT_RETRY_INTERVAL_SECS: u64 = 5;

/// 集群模式下解析安装包路径：CLI --package > checkpoint 缓存 > config.toml
/// installer_package/installer_url > 自动检测下载（按 primary 节点平台，假定集群内
/// 所有节点平台一致，下载后同一份包推送到各节点）。
pub(super) async fn resolve_cluster_package(
    args: &InstallArgs,
    installer: &InstallerSource,
    primary_runner: &dyn CommandRunner,
    cp: &mut ClusterCheckpoint,
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
        crate::ui::log_info(&format!(
            "[续] 跳过下载，使用已缓存安装包: {}",
            cached.display()
        ));
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
            crate::ui::log_info("自动检测 primary 节点平台并下载安装包...");
            let platform = detect_remote_platform(primary_runner).await;
            let handle = crate::download::fetch_dm_installer_for(&platform).await?;
            let cached = cache_package(&handle.path)?;
            cp.package_cache = Some(cached.to_string_lossy().into_owned());
            cp.save()?;
            Ok(cached)
        }
    }
}

fn node_ssh_credentials(node: &DwNode) -> SshCredentials {
    node.ssh.clone()
}

pub(super) async fn connect_all_nodes(cluster: &DwClusterConfig) -> Result<Vec<SshSession>> {
    let futs = cluster.nodes.iter().map(|node| async move {
        let creds = node_ssh_credentials(node);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockRunner;

    fn make_cp() -> ClusterCheckpoint {
        ClusterCheckpoint::new(1, "p1".into(), "p2".into(), &["h".to_string()])
    }

    #[tokio::test]
    async fn test_resolve_cluster_package_prefers_cli_package() {
        let args = InstallArgs {
            package: Some("/tmp/x.iso".into()),
            url: None,
        };
        let runner = MockRunner::new(vec![]);
        let mut cp = make_cp();
        let path = resolve_cluster_package(&args, &InstallerSource::Auto, &runner, &mut cp)
            .await
            .unwrap();
        assert_eq!(path, PathBuf::from("/tmp/x.iso"));
        assert!(runner.exec_log().is_empty(), "CLI 指定包时不应触碰节点");
    }

    #[tokio::test]
    async fn test_resolve_cluster_package_uses_local_file_from_config() {
        let args = InstallArgs {
            package: None,
            url: None,
        };
        let runner = MockRunner::new(vec![]);
        let mut cp = make_cp();
        let path = resolve_cluster_package(
            &args,
            &InstallerSource::LocalFile("/tmp/dm8.iso".into()),
            &runner,
            &mut cp,
        )
        .await
        .unwrap();
        assert_eq!(path, PathBuf::from("/tmp/dm8.iso"));
    }

    #[tokio::test]
    async fn test_resolve_cluster_package_skips_download_when_cached() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cached_pkg = tmp.path().join(".dm_cache_dm8.iso");
        std::fs::write(&cached_pkg, b"fake iso").unwrap();

        let args = InstallArgs {
            package: None,
            url: None,
        };
        let runner = MockRunner::new(vec![]);
        let mut cp = make_cp();
        cp.package_cache = Some(cached_pkg.to_string_lossy().into_owned());

        let resolved = resolve_cluster_package(&args, &InstallerSource::Auto, &runner, &mut cp)
            .await
            .unwrap();
        assert_eq!(resolved, cached_pkg);
        assert!(
            runner.exec_log().is_empty(),
            "缓存命中时不应触发平台探测: {:?}",
            runner.exec_log()
        );
    }
}
