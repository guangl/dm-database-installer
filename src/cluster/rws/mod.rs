use anyhow::Result;
use crate::config::CommonConfig;
use crate::config::cluster::ClusterSpecificConfig;

/// 读写分离集群部署入口。
/// 当前复用主备部署流程；读写路由配置待后续实现。
pub async fn run(common: CommonConfig, specific: ClusterSpecificConfig) -> Result<()> {
    crate::cluster::primary_standby::run(common, specific).await
}
