use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// dm-installer 的顶层 CLI 入口
#[derive(Parser)]
#[command(name = "dm-installer", version, about = "达梦数据库安装器")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// 支持的子命令集合
#[derive(Subcommand)]
pub enum Commands {
    /// 安装达梦数据库（读取当前目录 config.toml，自动判断单机/集群）
    Install(InstallArgs),
    /// 验证配置文件合法性（不执行安装）
    Validate(ValidateArgs),
    /// 生成配置文件模板
    Init(InitArgs),
    /// 更新 dm-installer 到最新版本
    SelfUpdate(SelfUpdateArgs),
}

/// install 子命令参数（配置从 config.toml 自动读取，无需 --config）
#[derive(clap::Args)]
pub struct InstallArgs {
    /// 本地安装包路径，覆盖 config.toml 中的 installer_package（仅单机）
    #[arg(long)]
    pub package: Option<PathBuf>,

    /// 自定义下载链接，覆盖 config.toml 中的 installer_url（仅单机）
    #[arg(long)]
    pub url: Option<String>,
}

/// validate 子命令参数
#[derive(clap::Args)]
pub struct ValidateArgs {
    /// 配置文件路径（默认读取当前目录 config.toml）
    pub config: Option<PathBuf>,
}

/// init 子命令参数
#[derive(clap::Args)]
pub struct InitArgs {
    #[command(subcommand)]
    pub kind: InitKind,
}

/// init 支持的配置类型
#[derive(Subcommand)]
pub enum InitKind {
    /// 生成单机安装配置模板（输出 config.toml + standalone.toml）
    Standalone(InitOutputArgs),
    /// 生成主备集群配置模板（即将支持）
    #[command(name = "primary-standby")]
    PrimaryStandby,
    /// 生成 DSC 共享存储集群配置模板（即将支持）
    Dsc,
    /// 生成 DPC 分布式集群配置模板（即将支持）
    Dpc,
}

/// self-update 子命令参数
#[derive(clap::Args)]
pub struct SelfUpdateArgs {
    /// 仅检查是否有新版本，不执行下载和替换
    #[arg(long)]
    pub check: bool,
}

/// init 子命令公共输出参数
#[derive(clap::Args)]
pub struct InitOutputArgs {
    /// 输出目录（默认为当前目录，生成 config.toml + 特有配置文件）
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    /// 覆盖已存在的文件
    #[arg(long)]
    pub force: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_install_no_args_parses() {
        let cli = Cli::try_parse_from(["dm-installer", "install"]).unwrap();
        let Commands::Install(args) = cli.command else {
            panic!("expected Install")
        };
        assert!(args.package.is_none());
    }

    #[test]
    fn test_install_with_package() {
        let cli =
            Cli::try_parse_from(["dm-installer", "install", "--package", "/tmp/x.iso"]).unwrap();
        let Commands::Install(args) = cli.command else {
            panic!("expected Install")
        };
        assert_eq!(args.package, Some(PathBuf::from("/tmp/x.iso")));
    }

    #[test]
    fn test_validate_defaults_to_no_path() {
        let cli = Cli::try_parse_from(["dm-installer", "validate"]).unwrap();
        let Commands::Validate(args) = cli.command else {
            panic!("expected Validate")
        };
        assert!(args.config.is_none());
    }

    #[test]
    fn test_validate_with_explicit_config() {
        let cli = Cli::try_parse_from(["dm-installer", "validate", "/etc/dm.toml"]).unwrap();
        let Commands::Validate(args) = cli.command else {
            panic!("expected Validate")
        };
        assert_eq!(args.config, Some(PathBuf::from("/etc/dm.toml")));
    }


}
