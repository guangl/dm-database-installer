use anyhow::Result;
use clap::Parser;

mod cli;
mod cmd;
mod config;
mod download;
mod install;
mod platform;
mod ssh;
mod ui;

#[tokio::main]
async fn main() -> Result<()> {
    let cli_args = cli::Cli::parse();

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
                    install::standalone::run(args, cfg.common, *specific).await
                }
                config::LoadedSpecific::Dw(cluster) => {
                    install::dw::run(args, cfg.common, &cluster).await
                }
            }
        }
        cli::Commands::SelfUpdate(args) => cmd::self_update::run(args.check).await,
        cli::Commands::Validate(args) => cmd::validate::run(args).await,
        cli::Commands::Init(args) => cmd::init::run(&args.kind),
    }
}
