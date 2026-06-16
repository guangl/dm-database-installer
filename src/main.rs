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
                cmd::guide::print_install();
                std::process::exit(1);
            });
            install::standalone::run(args, cfg.common, cfg.specific).await
        }
        cli::Commands::SelfUpdate(args) => cmd::self_update::run(args.check).await,
        cli::Commands::Validate(args) => cmd::validate::run(args).await,
        cli::Commands::Init(args) => cmd::init::run(&args.kind),
    }
}
