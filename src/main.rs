use anyhow::Result;
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
                    tracing::debug!(
                        nodes = cluster.nodes.len(),
                        "dispatching to dw cluster install"
                    );
                    install::dw::run(args, cfg.common, &cluster).await
                }
                config::LoadedSpecific::Dpc(cluster) => {
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
