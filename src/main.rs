use anyhow::Result;
use clap::Parser;
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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
    let log_cfg = config::load_log_config();
    let _guard = init_tracing(cli_args.verbose, &log_cfg);

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
        cli::Commands::Validate(args) => config::validate::run(args).await,
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

/// 初始化 tracing：始终输出到终端，若配置了 file 则同时写入文件（含回滚）。
/// 返回 WorkerGuard，必须保持到程序退出，否则文件输出会提前关闭。
fn init_tracing(verbose: bool, log_cfg: &config::LogConfig) -> Option<WorkerGuard> {
    let level = if verbose { "debug" } else { &log_cfg.level };

    if let Some(path) = &log_cfg.file {
        let dir = path.parent().unwrap_or(std::path::Path::new("."));
        let name = path.file_name().unwrap_or_default();

        let appender = match log_cfg.rotation {
            config::LogRotation::Daily => rolling::daily(dir, name),
            config::LogRotation::Hourly => rolling::hourly(dir, name),
            config::LogRotation::Never => rolling::never(dir, name),
        };
        let (non_blocking, guard) = tracing_appender::non_blocking(appender);

        tracing_subscriber::registry()
            .with(EnvFilter::new(level))
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).with_ansi(false))
            .init();

        if log_cfg.max_files > 0 && log_cfg.rotation != config::LogRotation::Never {
            prune_old_logs(dir, &name.to_string_lossy(), log_cfg.max_files);
        }
        Some(guard)
    } else {
        tracing_subscriber::registry()
            .with(EnvFilter::new(level))
            .with(tracing_subscriber::fmt::layer())
            .init();
        None
    }
}

/// 删除日志目录下超出保留数量的旧轮转文件，按修改时间排序（最旧的先删）。
fn prune_old_logs(dir: &std::path::Path, prefix: &str, max_files: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };

    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|name| name.starts_with(prefix) && name != prefix)
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((mtime, e.path()))
        })
        .collect();

    if files.len() <= max_files {
        return;
    }

    files.sort_by_key(|(mtime, _)| *mtime);
    let to_delete = files.len() - max_files;
    for (_, path) in files.into_iter().take(to_delete) {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!("删除旧日志文件失败 {}: {e}", path.display());
        } else {
            tracing::debug!("已删除旧日志文件: {}", path.display());
        }
    }
}
