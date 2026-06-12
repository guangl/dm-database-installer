use std::path::PathBuf;
use clap::{Parser, Subcommand};

/// dm-installer 的顶层 CLI 入口
#[derive(Parser)]
#[command(name = "dm-installer", version, about = "达梦数据库安装器")]
pub struct Cli {
    /// 启用 verbose 日志输出
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// 支持的子命令集合
#[derive(Subcommand)]
pub enum Commands {
    /// 安装达梦数据库单机实例
    Install(InstallArgs),
    /// 验证 TOML 配置文件合法性（不执行安装）
    Validate(ValidateArgs),
    /// 生成 shell 补全脚本
    Completions {
        /// 目标 shell 类型（bash/zsh/fish 等）
        shell: clap_complete::Shell,
    },
}

/// install 子命令参数
#[derive(clap::Args)]
pub struct InstallArgs {
    /// 本地 ISO 安装包路径
    #[arg(long)]
    pub package: Option<PathBuf>,

    /// 可选的 SHA-256 校验和（十六进制字符串）
    #[arg(long)]
    pub checksum: Option<String>,

    /// 跳过所有交互确认（curl | sh 模式使用）
    #[arg(long)]
    pub defaults: bool,

    /// 跳过确认，等同于 --defaults
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// validate 子命令参数
#[derive(clap::Args)]
pub struct ValidateArgs {
    /// TOML 配置文件路径
    #[arg(long)]
    pub config: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_install_args_defaults() {
        // 验证 --defaults flag 解析为 true，--package 为 None
        let cli = Cli::try_parse_from(["dm-installer", "install", "--defaults"]).unwrap();
        let Commands::Install(args) = cli.command else {
            panic!("expected Install command");
        };
        assert!(args.defaults, "--defaults 应解析为 true");
        assert!(args.package.is_none(), "--package 应为 None");
        assert!(args.checksum.is_none(), "--checksum 应为 None");
        assert!(!args.yes, "--yes 应为 false");
    }

    #[test]
    fn test_install_args_with_package() {
        // 验证 --package 路径正确解析为 Some(PathBuf)
        let cli = Cli::try_parse_from(["dm-installer", "install", "--package", "/tmp/x.iso"]).unwrap();
        let Commands::Install(args) = cli.command else {
            panic!("expected Install command");
        };
        assert_eq!(
            args.package,
            Some(PathBuf::from("/tmp/x.iso")),
            "--package 应解析为正确路径"
        );
        assert!(!args.defaults, "--defaults 应为 false");
    }

    #[test]
    fn test_validate_args_config() {
        // 验证 --config 参数必填且正确解析
        let cli = Cli::try_parse_from(["dm-installer", "validate", "--config", "/etc/dm.toml"]).unwrap();
        let Commands::Validate(args) = cli.command else {
            panic!("expected Validate command");
        };
        assert_eq!(
            args.config,
            PathBuf::from("/etc/dm.toml"),
            "--config 应解析为正确路径"
        );
    }

    #[test]
    fn test_validate_requires_config() {
        // validate 子命令不带 --config 应报错
        let result = Cli::try_parse_from(["dm-installer", "validate"]);
        assert!(result.is_err(), "validate 不带 --config 应解析失败");
    }

    #[test]
    fn test_yes_short_flag() {
        // 验证 -y 短参数与 --yes 等效
        let cli = Cli::try_parse_from(["dm-installer", "install", "-y"]).unwrap();
        let Commands::Install(args) = cli.command else {
            panic!("expected Install command");
        };
        assert!(args.yes, "-y 应解析为 yes=true");
    }
}
