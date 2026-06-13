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
    /// 集群部署子命令
    Cluster(ClusterArgs),
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

    /// TOML 配置文件路径（可选；未提供时使用内置默认参数）
    #[arg(long)]
    pub config: Option<PathBuf>,
}

/// validate 子命令参数
#[derive(clap::Args)]
pub struct ValidateArgs {
    /// TOML 配置文件路径
    #[arg(long)]
    pub config: PathBuf,
}

/// cluster 子命令参数
#[derive(clap::Args)]
pub struct ClusterArgs {
    #[command(subcommand)]
    pub command: ClusterSubcommand,
}

/// cluster 支持的子命令
#[derive(Subcommand)]
pub enum ClusterSubcommand {
    /// 部署主备集群
    Deploy(ClusterDeployArgs),
}

/// cluster deploy 子命令参数
#[derive(clap::Args)]
pub struct ClusterDeployArgs {
    /// 集群 TOML 配置文件路径（必填）
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

    #[test]
    fn test_install_args_with_config() {
        // 验证 --config 路径正确解析为 Some(PathBuf)
        let cli = Cli::try_parse_from(["dm-installer", "install", "--config", "/etc/dm.toml"]).unwrap();
        let Commands::Install(args) = cli.command else {
            panic!("expected Install command");
        };
        assert_eq!(
            args.config,
            Some(PathBuf::from("/etc/dm.toml")),
            "--config 应解析为 Some(/etc/dm.toml)"
        );
    }

    #[test]
    fn test_install_args_config_default_none() {
        // 未提供 --config 时，config 字段应为 None
        let cli = Cli::try_parse_from(["dm-installer", "install", "--defaults"]).unwrap();
        let Commands::Install(args) = cli.command else {
            panic!("expected Install command");
        };
        assert!(args.config.is_none(), "未提供 --config 时应为 None");
    }

    #[test]
    fn test_cluster_deploy_args_config() {
        let cli = Cli::try_parse_from([
            "dm-installer", "cluster", "deploy", "--config", "/etc/cluster.toml",
        ])
        .unwrap();
        let Commands::Cluster(args) = cli.command else {
            panic!("expected Cluster command");
        };
        let ClusterSubcommand::Deploy(deploy_args) = args.command;
        assert_eq!(
            deploy_args.config,
            PathBuf::from("/etc/cluster.toml"),
            "--config 应解析为正确路径"
        );
    }

    #[test]
    fn test_cluster_deploy_requires_config() {
        let result = Cli::try_parse_from(["dm-installer", "cluster", "deploy"]);
        assert!(result.is_err(), "cluster deploy 不带 --config 应解析失败");
    }

    #[test]
    fn test_cluster_requires_subcommand() {
        let result = Cli::try_parse_from(["dm-installer", "cluster"]);
        assert!(result.is_err(), "cluster 不带子命令应解析失败");
    }

    #[test]
    fn test_install_args_config_and_defaults_combined() {
        // --config 与 --defaults 正交，可同时指定（D-03）
        let cli = Cli::try_parse_from([
            "dm-installer", "install", "--config", "/etc/dm.toml", "--defaults"
        ]).unwrap();
        let Commands::Install(args) = cli.command else {
            panic!("expected Install command");
        };
        assert_eq!(args.config, Some(PathBuf::from("/etc/dm.toml")), "config 应为 Some");
        assert!(args.defaults, "defaults 应为 true");
    }

    #[test]
    fn test_install_windows_placeholder_parses() {
        // PLAT-04: install-windows 无参数应解析为 InstallWindows(args)，config 为 None
        let cli = Cli::try_parse_from(["dm-installer", "install-windows"]).unwrap();
        let Commands::InstallWindows(args) = cli.command else {
            panic!("expected InstallWindows command");
        };
        assert!(args.config.is_none(), "--config 应为 None");
    }

    #[test]
    fn test_install_windows_with_config() {
        // PLAT-04: install-windows --config 应解析为 Some(PathBuf)
        let cli = Cli::try_parse_from([
            "dm-installer", "install-windows", "--config", "/etc/dm.toml",
        ])
        .unwrap();
        let Commands::InstallWindows(args) = cli.command else {
            panic!("expected InstallWindows command");
        };
        assert_eq!(
            args.config,
            Some(PathBuf::from("/etc/dm.toml")),
            "--config 应解析为 Some(/etc/dm.toml)"
        );
    }
}
