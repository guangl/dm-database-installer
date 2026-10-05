use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dm_database_installer::run(dm_database_installer::cli::Cli::parse(), false).await
}
