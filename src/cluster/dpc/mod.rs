use anyhow::Result;
use crate::config::CommonConfig;
use crate::config::cluster::ClusterSpecificConfig;

/// DPC 集群部署入口（待实现）。
pub async fn run(_common: CommonConfig, _specific: ClusterSpecificConfig) -> Result<()> {
    anyhow::bail!("DPC 集群部署尚未实现")
}
