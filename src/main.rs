use anyhow::{bail, Result};
use clap::{CommandFactory, Parser};
use tracing_subscriber::EnvFilter;

mod cli;
mod cmd;
mod config;
mod download;
mod install;
mod platform;
mod ssh;
mod ui;

/// 集群安装（DW/DPC）编排代码已实现但尚未经过实际环境测试，默认拦截。
/// 需要在测试环境放行时，设置环境变量 `DM_ALLOW_UNTESTED_CLUSTER=1`。
fn ensure_cluster_install_allowed(kind: &str) -> Result<()> {
    if std::env::var("DM_ALLOW_UNTESTED_CLUSTER").as_deref() == Ok("1") {
        tracing::warn!("DM_ALLOW_UNTESTED_CLUSTER=1，放行未测试的{kind}安装");
        return Ok(());
    }
    bail!(
        "{kind}安装尚未经过测试，暂不支持。\n\
         如需在测试环境强制运行，请设置环境变量 DM_ALLOW_UNTESTED_CLUSTER=1 后重试。"
    );
}

fn init_tracing(verbose: u8) {
    let default_level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .with_target(false)
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli_args = cli::Cli::parse();
    init_tracing(cli_args.verbose);

    match &cli_args.command {
        cli::Commands::Install(args) => {
            let cfg = config::load_config().unwrap_or_else(|e| {
                eprintln!("{e}");
                if !std::path::Path::new(config::CONFIG_FILE).exists() {
                    cmd::guide::print_install();
                } else {
                    eprintln!("\n请运行: dm_installer init standalone");
                }
                std::process::exit(1);
            });
            match cfg.specific {
                config::LoadedSpecific::Standalone(specific) => {
                    tracing::debug!("dispatching to standalone install");
                    install::standalone::run(args, cfg.common, *specific).await
                }
                config::LoadedSpecific::Dw(cluster) => {
                    ensure_cluster_install_allowed("主备集群（DW）")?;
                    tracing::debug!(
                        nodes = cluster.nodes.len(),
                        "dispatching to dw cluster install"
                    );
                    install::dw::run(args, cfg.common, &cluster).await
                }
                config::LoadedSpecific::Dpc(cluster) => {
                    ensure_cluster_install_allowed("DPC 分布式集群")?;
                    tracing::debug!(
                        nodes = cluster.nodes.len(),
                        "dispatching to dpc cluster install"
                    );
                    install::dpc::run(args, cfg.common, &cluster).await
                }
            }
        }
        cli::Commands::SelfUpdate(args) => cmd::self_update::run(args.check).await,
        cli::Commands::Validate(args) => cmd::validate::run(args).await,
        cli::Commands::Init(args) => cmd::init::run(&args.kind),
        cli::Commands::Completions(args) => {
            clap_complete::generate(
                args.shell,
                &mut cli::Cli::command(),
                "dm_installer",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    }
}
