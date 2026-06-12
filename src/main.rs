use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};

mod cli;
mod config;
mod download;
mod install;
mod ui;

/// dm-database-installer 主入口。
/// 解析 CLI 参数，初始化 tracing 日志，dispatch 到对应子命令。
#[tokio::main]
async fn main() -> Result<()> {
    let cli_args = cli::Cli::parse();

    // 根据 --verbose flag 配置 tracing EnvFilter
    let filter = if cli_args.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };
    fmt().with_env_filter(filter).init();

    match &cli_args.command {
        cli::Commands::Install(args) => install::run(args).await,
        cli::Commands::Validate(args) => config::validate::run(args),
        cli::Commands::Completions { shell } => {
            use clap::CommandFactory;
            use clap_complete::generate;
            let mut cmd = cli::Cli::command();
            generate(*shell, &mut cmd, "dm-installer", &mut std::io::stdout());
            Ok(())
        }
    }
}
