use anyhow::Result;

pub mod preflight;
pub mod ssh;
pub mod templates;

/// 集群部署入口（占位 stub，由 Plan 03 实现完整编排逻辑）。
///
/// 当前签名无参数；Plan 03 改为 `pub async fn run(args: &crate::cli::ClusterDeployArgs) -> Result<()>`。
pub async fn run() -> Result<()> {
    unimplemented!("cluster::run 由 Plan 03 实现")
}
