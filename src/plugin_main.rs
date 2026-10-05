use clap::{CommandFactory, FromArgMatches};
use dm_plugin_sdk::{Context, Plugin, PluginResult};

struct Installer;

impl Plugin for Installer {
    fn run(&self, context: Context) -> PluginResult {
        let command = dm_database_installer::cli::Cli::command().bin_name("dm installer");
        let args = std::iter::once(std::ffi::OsString::from("dm installer")).chain(context.args);
        let matches = match command.try_get_matches_from(args) {
            Ok(matches) => matches,
            Err(error) => {
                let code = error.exit_code();
                error.print()?;
                return Ok(code);
            }
        };
        let cli = dm_database_installer::cli::Cli::from_arg_matches(&matches)?;
        tokio::runtime::Runtime::new()?
            .block_on(dm_database_installer::run(cli, true))
            .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { error.into() })?;
        Ok(0)
    }
}

fn main() {
    dm_plugin_sdk::run(Installer);
}
