use anyhow::Result;
use crate::config::{CommonConfig, InstallType};
use crate::config::cluster::ClusterSpecificConfig;

pub mod checkpoint;
pub mod deploy;
pub mod dpc;
pub mod dsc;
pub mod health;
pub mod preflight;
pub mod primary_standby;
pub mod rws;
pub mod templates;

/// 根据 install_type 分派到对应集群部署入口。
/// common 和 specific 已由调用方从配置文件加载并验证。
pub async fn run(
    install_type: InstallType,
    common: CommonConfig,
    specific: ClusterSpecificConfig,
) -> Result<()> {
    tracing::info!("开始集群部署: {:?}", install_type);
    match install_type {
        InstallType::PrimaryStandby => primary_standby::run(common, specific).await,
        InstallType::Rws => rws::run(common, specific).await,
        InstallType::Dsc => dsc::run(common, specific).await,
        InstallType::Dpc => dpc::run(common, specific).await,
        InstallType::Standalone => unreachable!("standalone 不通过 cluster::run 分派"),
    }
}
