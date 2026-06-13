use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};

mod cli;
mod cluster;
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
        cli::Commands::Cluster(args) => match &args.command {
            cli::ClusterSubcommand::Deploy(deploy_args) => cluster::run(deploy_args).await,
        },
        cli::Commands::InstallWindows(_args) => {
            // PLAT-04 spike: setup.exe /q /XML <path> 集成待完成
            // DM Windows 安装包 URL 需从 eco.dameng.com 单独验证
            // 实现方式说明：D-07 授权采用 eprintln + exit(1)，等价于 CONTEXT.md L45 示意的 placeholder
            // 优势：避免 panic backtrace；用户看到明确的中文错误信息
            eprintln!("[WARN] Windows 目标机安装尚未实现（PLAT-04 spike 待完成）");
            eprintln!("请参考: https://eco.dameng.com/ 手动获取 Windows 安装包");
            std::process::exit(1);
        }
        cli::Commands::Completions { shell } => {
            use clap::CommandFactory;
            use clap_complete::generate;
            let mut cmd = cli::Cli::command();
            generate(*shell, &mut cmd, "dm-installer", &mut std::io::stdout());
            Ok(())
        }
    }
}
