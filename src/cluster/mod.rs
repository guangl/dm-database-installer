use anyhow::Result;

pub mod deploy;
pub mod dpc;
pub mod dsc;
pub mod health;
pub mod preflight;
pub mod primary_standby;
pub mod rws;
pub mod templates;

/// 根据集群配置中的 type 字段分派到对应模式的部署入口。
pub async fn run(args: &crate::cli::ClusterDeployArgs) -> Result<()> {
    let Some(config_path) = &args.config else {
        crate::guide::print_cluster();
        anyhow::bail!("缺少 --config 参数");
    };

    use crate::config::cluster::{load_cluster_config, ClusterType};
    let config = load_cluster_config(config_path)
        .map_err(|e| anyhow::anyhow!("加载集群配置失败: {}: {}", config_path.display(), e))?;
    match config.cluster.cluster_type {
        ClusterType::PrimaryStandby => primary_standby::run(args).await,
        ClusterType::Rws => rws::run(args).await,
        ClusterType::Dsc => dsc::run(args).await,
    }
}
