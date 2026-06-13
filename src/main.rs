use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};

mod cli;
mod cluster;
mod common;
mod config;
mod guide;
mod standalone;

/// dm-database-installer 主入口。
#[tokio::main]
async fn main() -> Result<()> {
    let cli_args = cli::Cli::parse();

    let filter = if cli_args.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };
    fmt().with_env_filter(filter).init();

    match &cli_args.command {
        cli::Commands::Install(args) => {
            let cfg = config::load_config().unwrap_or_else(|e| {
                eprintln!("{e}");
                guide::print_install();
                std::process::exit(1);
            });
            match cfg {
                config::LoadedConfig::Standalone { common, specific } => {
                    standalone::run(args, common, specific).await
                }
                config::LoadedConfig::Cluster { common, specific, install_type } => {
                    cluster::run(install_type, common, specific).await
                }
            }
        }
        cli::Commands::Validate(args) => config::validate::run(args),
        cli::Commands::Init(args) => config::init::run(&args.kind),
        cli::Commands::Completions { shell } => {
            use clap::CommandFactory;
            use clap_complete::generate;
            let mut cmd = cli::Cli::command();
            generate(*shell, &mut cmd, "dm-installer", &mut std::io::stdout());
            Ok(())
        }
    }
}
