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
    /// 生成默认配置文件模板
    Init(InitArgs),
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

/// init 子命令参数
#[derive(clap::Args)]
pub struct InitArgs {
    #[command(subcommand)]
    pub kind: InitKind,
}

/// init 支持的配置类型
#[derive(Subcommand)]
pub enum InitKind {
    /// 生成单机安装配置模板
    Standalone(InitOutputArgs),
    /// 生成集群安装配置模板（需指定集群类型）
    Cluster(ClusterInitArgs),
}

/// cluster init 子命令参数
#[derive(clap::Args)]
pub struct ClusterInitArgs {
    #[command(subcommand)]
    pub kind: ClusterInitKind,
}

/// 集群类型
#[derive(Subcommand)]
pub enum ClusterInitKind {
    /// 主备集群（Primary-Standby）
    PrimaryStandby(InitOutputArgs),
    /// 读写分离集群（基于主备，备节点承担只读查询）
    Rws(InitOutputArgs),
    /// 共享存储集群 DSC（Data Sharing Cluster，多实例共享 SAN/NFS）
    Dsc(InitOutputArgs),
}

/// init 子命令公共输出参数
#[derive(clap::Args)]
pub struct InitOutputArgs {
    /// 输出文件路径
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
    fn test_install_args_with_package() {
        let cli = Cli::try_parse_from(["dm-installer", "install", "--package", "/tmp/x.iso"]).unwrap();
        let Commands::Install(args) = cli.command else {
            panic!("expected Install command");
        };
        assert_eq!(args.package, Some(PathBuf::from("/tmp/x.iso")));
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
        let cli = Cli::try_parse_from(["dm-installer", "install"]).unwrap();
        let Commands::Install(args) = cli.command else {
            panic!("expected Install command");
        };
        assert!(args.config.is_none());
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

}
