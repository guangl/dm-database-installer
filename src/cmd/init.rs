use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::cli::{InitKind, InitOutputArgs};

pub fn run(kind: &InitKind) -> Result<()> {
    match kind {
        InitKind::Standalone(args) => {
            let dir = output_dir(args);
            let wrote_common =
                write_template(&dir.join("config.toml"), args.force, STANDALONE_COMMON)?;
            let wrote_specific = write_template(
                &dir.join("standalone.toml"),
                args.force,
                STANDALONE_SPECIFIC,
            )?;
            if wrote_common || wrote_specific {
                println!("已生成单机配置模板:");
                if wrote_common {
                    println!("  config.toml      — 通用配置（type、安装包路径等）");
                }
                if wrote_specific {
                    println!("  standalone.toml  — 单机特有配置（端口、路径、字符集等）");
                }
                println!("编辑后使用: dm_installer install");
            } else {
                println!("配置文件已存在，无需覆盖。使用 --force 强制重新生成。");
            }
            Ok(())
        }
        InitKind::Dw(args) => {
            let dir = output_dir(args);
            let wrote_common = write_template(&dir.join("config.toml"), args.force, DW_COMMON)?;
            let wrote_specific = write_template(&dir.join("dw.toml"), args.force, DW_SPECIFIC)?;
            if wrote_common || wrote_specific {
                println!("已生成主备集群配置模板:");
                if wrote_common {
                    println!("  config.toml — 通用配置（type、安装包路径等）");
                }
                if wrote_specific {
                    println!("  dw.toml     — 主备集群节点配置（角色、网络端口、SSH 凭证等）");
                }
                println!("编辑后使用: dm_installer validate 校验，再 dm_installer install 部署");
            } else {
                println!("配置文件已存在，无需覆盖。使用 --force 强制重新生成。");
            }
            Ok(())
        }
        InitKind::Rws | InitKind::Dsc | InitKind::Dpc => {
            let mode = match kind {
                InitKind::Rws => "读写分离集群（rws）",
                InitKind::Dsc => "DSC 共享存储集群（dsc）",
                InitKind::Dpc => "DPC 分布式集群（dpc）",
                _ => unreachable!(),
            };
            println!("{} 配置模板即将支持，请关注后续版本。", mode);
            println!("当前可使用: dm_installer init standalone / dm_installer init dw");
            Ok(())
        }
    }
}

fn output_dir(args: &InitOutputArgs) -> PathBuf {
    args.output.clone().unwrap_or_else(|| PathBuf::from("."))
}

/// 返回 true 表示实际写入了文件，false 表示跳过（已存在且未 force）
fn write_template(path: &Path, force: bool, content: &str) -> Result<bool> {
    if path.exists() && !force {
        println!("跳过已存在的文件: {}", path.display());
        return Ok(false);
    }
    std::fs::write(path, content)
        .map_err(|e| anyhow::anyhow!("无法写入配置文件 {}: {}", path.display(), e))?;
    Ok(true)
}

const STANDALONE_COMMON: &str = r#"# 达梦数据库单机安装 — 通用配置
# 使用方式: dm_installer install

type = "standalone"

# ─── 安装包来源（三选一，都不填则自动检测下载）────────────────
# 本地文件路径
# installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"
# 自定义下载链接
# installer_url = "https://download.example.com/dm8.zip"
"#;

const STANDALONE_SPECIFIC: &str = r#"# 达梦数据库单机安装 — 特有配置（standalone.toml）
# 注意：SYSDBA / SYSAUDITOR 密码在安装时由终端提示输入，不写入此文件
#
# ─── 速览：通常只需要确认/修改这两处 ─────────────────────────
#   [instance] instance_name / port  — 实例名与监听端口
#   [backup]   backup_path           — 备份目录，必填
# 其余字段（[install]/[archive] 路径、各类阈值）均有默认值，按需调整即可。

[install]
install_path = "/home/dmdba/dmdbms"
data_path = "/home/dmdba/dmdbms/data"

[instance]
instance_name = "DMSERVER"
port = 5236
# 页大小（KB），可选值：4 / 8 / 16 / 32
page_size = 32
# 字符集：0=GB18030  1=UTF-8  2=EUC-KR
charset = 1
case_sensitive = true
# 区段大小（页数），可选值：16 / 32
extent_size = 32

# ─── 本地归档配置 ──────────────────────────────────────────
# 安装完成后会在线开启本地归档（MOUNT → ARCHIVELOG → ADD ARCHIVELOG → OPEN），
# 无需重启 dmserver，以下参数均可省略走默认值。
[archive]
# arch_path = "/home/dmdba/dmdbms/data/arch"  # 不填则默认为 data_path/arch
file_size   = 1024  # 单归档文件大小（MB）
# space_limit = 0    # 归档空间上限（MB），不填则默认为磁盘总容量的 20%；显式填 0 = 无限

# ─── 备份作业配置 ──────────────────────────────────────────
# 安装完成后会自动创建达梦作业系统中的全备/增量备份/清理作业（写入 backup_path）。
[backup]
# 数据库备份目录，必须配置（用于创建备份作业）
backup_path = "/home/dmdba/dmdbms/backup"
retain_days = 15 # 备份保留天数，至少 15 天
# 全量备份间隔天数：
#   1 = 每天只做全量备份，不创建增量备份作业
#   7（默认）= 与自然周对齐，全量固定在每周六，增量固定在周日至周五
#   其他 N = 全量每 N 天一次，其余天做增量（与全量同一天会同时执行，增量内容很少）
full_backup_interval_days = 7
full_backup_time = "02:00:00" # 全量备份时间（HH:MM:SS）
incr_backup_time = "02:00:00" # 增量备份时间（HH:MM:SS，full_backup_interval_days=1 时不生效）
clean_time        = "05:00:00" # 过期备份清理时间（每天，HH:MM:SS）

# ─── SSH 远程安装目标（可选）────────────────────────────────
# 填写后将通过 SSH 在目标服务器上安装，host 为本机时自动退化为本地安装。
# password 不填则运行时提示输入。
# [ssh_target]
# host = "192.168.1.100"
# ssh_port = 22
# user = "root"
# password = "your_ssh_password"
# max_retries = 3
# retry_interval_secs = 5
"#;

const DW_COMMON: &str = r#"# 达梦数据库主备集群安装 — 通用配置
# 使用方式: dm_installer install

type = "dw"

# ─── 安装包来源（三选一，都不填则自动检测 primary 节点平台并下载）──
# 工具会把这个文件推送到各节点后再静默安装。
# 本地文件路径
# installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"
# 自定义下载链接
# installer_url = "https://download.example.com/dm8.zip"
"#;

const DW_SPECIFIC: &str = r#"# 达梦数据库主备集群安装 — 节点配置（dw.toml）
# 注意：SYSDBA / SYSAUDITOR 密码在安装时由终端提示输入，不写入此文件
#
# ─── 速览：通常只需要修改这几处（其余均有默认值）──────────────
#   每个 [[nodes]]：host / instance_name / [nodes.ssh] 凭证
#   primary 节点：  [nodes.backup] backup_path  — 必填
#   standby 节点：  sync_mode                   — 选填，默认 realtime（见该节点注释）
#
# ─── 可选配置 section 一览（全部省略则使用括号内默认值）───────
#   [mal]     dmmal.ini    MAL 通信层参数（检测间隔/超时/缓冲区/压缩）
#   [watcher] dmwatcher.ini 守护进程参数（切换模式默认 MANUAL、故障确认时间等）
#   [arch]    dmarch.ini   归档参数（本地归档大小/空间上限默认自动 20%）
#   [monitor] dmmonitor.ini 确认监视器日志参数
#
# TOML 约束：oguid / mon_confirm 等顶层裸键必须在 [[nodes]] 之前声明；
#            [mal] / [watcher] / [arch] / [monitor] 为独立 section，可放在 [[nodes]] 之后。

# ── 集群标识（顶层裸键，必须在 [[nodes]] 前）────────────────────────────
# oguid = 20260623   # 全局唯一标识，范围 0-2147483647；省略则默认当天 YYYYMMDD
# mon_confirm = true   # true=确认监视（参与自动切换仲裁），false=通知监视

# ════════════════════════════════════════════════════════════════════════
# 节点配置（必填）
# ════════════════════════════════════════════════════════════════════════

# ── 主库节点 ─────────────────────────────────────────────────────────────
[[nodes]]
role          = "primary"
host          = "192.168.1.10"
instance_name = "DM01"
# install_path  = "/home/dmdba/dmdbms"
# data_path     = "/home/dmdba/dmdbms/data"
# arch_path     = "/home/dmdba/dmdbms/data/arch"  # 默认为 data_path/arch
# port          = 5236
# mal_port      = 5237
# dw_port       = 5238
# inst_dw_port  = 5239
# page_size     = 32    # 页大小（KB）：4 / 8 / 16 / 32
# charset       = 1     # 0=GB18030  1=UTF-8  2=EUC-KR
# case_sensitive = true
# extent_size   = 32    # 区段大小（页数）：16 / 32

# SSH 登录凭证（用于推送安装包、执行远程命令）
[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
# password = "xxx"   # 不填则使用 identity_file；两者至少配置一个

# 备份作业（备库无需配置，主库会自动将作业同步过去）
[nodes.backup]
backup_path = "/home/dmdba/dmbackup"
# retain_days               = 15          # 备份保留天数
# full_backup_interval_days = 7           # 全量备份间隔（天）
# full_backup_time          = "02:00:00"  # 全量备份执行时间
# incr_backup_time          = "02:00:00"  # 增量备份执行时间
# clean_time                = "05:00:00"  # 过期备份清理时间

# ── 备库节点 ─────────────────────────────────────────────────────────────
[[nodes]]
role          = "standby"
host          = "192.168.1.11"
instance_name = "DM02"
# sync_mode = "realtime"   # 备库类型：realtime（默认，REALTIME，本项目主备故障切换对采用此类型）
                           # / sync（SYNC，同步备库，主库等待该备库确认）
                           # / async（ASYNC，异步备库，主库不等待确认，定时通过 arch_timer_name 触发归档）
# arch_timer_name = "RT_TIMER"   # 仅 sync_mode = async 时生效，引用 DM 内置定时器，默认 "RT_TIMER"
# 以下字段含义、单位、默认值同上方主库节点注释，此处不重复：
# install_path / data_path / arch_path / port / mal_port / dw_port / inst_dw_port
# / page_size / charset / case_sensitive / extent_size
# install_path  = "/home/dmdba/dmdbms"
# data_path     = "/home/dmdba/dmdbms/data"
# arch_path     = "/home/dmdba/dmdbms/data/arch"
# port          = 5236
# mal_port      = 5237
# dw_port       = 5238
# inst_dw_port  = 5239
# page_size     = 32
# charset       = 1
# case_sensitive = true
# extent_size   = 32

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
# 备库无需配置 [nodes.backup]，备份作业由主库同步过来

# ════════════════════════════════════════════════════════════════════════
# 配置文件参数（选填，均有默认值）
# 以下三个配置文件在主备所有节点上内容完全一致，统一在此处配置。
# ════════════════════════════════════════════════════════════════════════

# ── MAL 通信层（dmmal.ini）───────────────────────────────────────────────
# 每节点的 MAL_INST_NAME / MAL_HOST / MAL_PORT 等由 [[nodes]] 自动推导，无需手动填写
# [mal]
# mal_check_interval     = 60   # MAL 链路检测间隔（秒），0=禁用，范围 0-1800
# mal_conn_fail_interval = 60   # 判定连接失败的时长阈值（秒），范围 2-1800
# mal_login_timeout      = 60   # 节点间登录超时（秒），范围 3-1800
# mal_buf_size           = 100  # 单连接缓冲区上限（MB），0=不限
# mal_sys_buf_size       = 0    # 系统全局 MAL 内存上限（MB），0=不限
# mal_compress_level     = 0    # 压缩级别：0=不压缩，1-9=lz，10=snappy
#                               # 注意：所有节点必须配置相同的压缩级别

# ── 数据守护（dmwatcher.ini）─────────────────────────────────────────────
# [watcher]
# dw_mode           = "MANUAL"   # MANUAL（默认，需人工介入）/ AUTO（故障自动切换）
# dw_error_time     = 60         # 守护进程故障确认时间（秒），范围 3-1800
# inst_error_time   = 60         # 数据库实例故障确认时间（秒），范围 3-1800
# inst_recover_time = 60         # 备库恢复检测间隔（秒），范围 3-86400
# inst_auto_restart = 0          # 实例崩溃后自动重启：0=否，1=是
# inst_restart_cnt  = 0          # 最大连续重启次数，0=不限制
# dw_open_force_timeout = 0      # MANUAL 模式无监视器时强制 Open 超时（秒），0=永久等待
# dw_failover_force = 1          # 无监视器时允许主库强制 Open：1=允许，0=禁止
# dw_reconnect      = 1          # 断链重连策略：0=不重连，1=重连继续守护，2=重连降为 OPEN
# rlog_send_threshold   = 0      # 实时归档发送延迟告警阈值（秒），0=不告警
# rlog_apply_threshold  = 0      # 备库日志应用延迟告警阈值（秒），0=不告警
# inst_service_ip_check = 0      # 检测主库对外服务 IP 可达性：0=不检测，1=检测

# ── 日志归档（dmarch.ini）────────────────────────────────────────────────
# [arch]
# arch_wait_apply    = 1     # 同步备库是否等待日志应用后再回应主库：1=等待，0=不等待
# arch_reserve_time  = 0     # 本地归档保留时长（分钟），0=不自动清理
# arch_send_policy   = 0     # 主库发送策略：0=立即等待备库响应，1=先写本地再发送
# arch_recover_time  = 60    # 同步备库归档状态检测间隔（秒），范围 1-86400
# arch_file_size     = 1024  # 本地归档单文件大小（MB），范围 64-2048
# arch_space_limit   = 20480 # 本地归档总空间上限（MB）：不填=自动取磁盘总容量的 20%
                              # （探测失败时退回默认值 20480，即 20GB），0=不限

# ── 确认监视器（dmmonitor.ini）───────────────────────────────────────────
# [monitor]
# mon_log_path        = "."  # 监视器日志输出目录
# mon_log_interval    = 60   # 日志刷新间隔（秒），范围 1-600
# mon_log_file_size   = 32   # 单个日志文件大小上限（MB），范围 1-2048
# mon_log_space_limit = 4096 # 日志总空间上限（MB），0=不限
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn output_args_in(dir: &TempDir, force: bool) -> InitOutputArgs {
        InitOutputArgs {
            output: Some(dir.path().to_path_buf()),
            force,
        }
    }

    #[test]
    fn test_standalone_creates_two_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        assert!(
            dir.path().join("config.toml").exists(),
            "应生成 config.toml"
        );
        assert!(
            dir.path().join("standalone.toml").exists(),
            "应生成 standalone.toml"
        );
    }

    #[test]
    fn test_standalone_common_has_type_field() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(
            content.contains("type = \"standalone\""),
            "通用配置应含 type = \"standalone\""
        );
    }

    #[test]
    fn test_standalone_specific_has_install_fields() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("standalone.toml")).unwrap();
        assert!(
            content.contains("install_path"),
            "特有配置应含 install_path"
        );
        assert!(content.contains("port = 5236"), "特有配置应含默认端口");
    }

    #[test]
    fn test_standalone_templates_are_valid_toml() {
        toml::from_str::<toml::Value>(STANDALONE_COMMON).expect("通用模板应为合法 TOML");
        toml::from_str::<toml::Value>(STANDALONE_SPECIFIC).expect("单机特有模板应为合法 TOML");
    }

    #[test]
    fn test_skips_existing_files_without_force() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        // 第二次运行不报错，只跳过已存在文件
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
    }

    #[test]
    fn test_partial_init_creates_missing_file() {
        let dir = TempDir::new().unwrap();
        // 只预先创建 config.toml
        std::fs::write(dir.path().join("config.toml"), "type = \"standalone\"\n").unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        // standalone.toml 应该被创建
        assert!(
            dir.path().join("standalone.toml").exists(),
            "standalone.toml 应被创建"
        );
    }

    #[test]
    fn test_force_overwrites_existing_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, true))).unwrap();
        assert!(dir.path().join("standalone.toml").exists());
    }

    #[test]
    fn test_dw_creates_two_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Dw(output_args_in(&dir, false))).unwrap();
        assert!(dir.path().join("config.toml").exists(), "应生成 config.toml");
        assert!(dir.path().join("dw.toml").exists(), "应生成 dw.toml");
    }

    #[test]
    fn test_dw_common_has_type_field() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Dw(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(content.contains("type = \"dw\""), "通用配置应含 type = \"dw\"");
    }

    #[test]
    fn test_dw_templates_are_valid_toml_and_pass_validation() {
        toml::from_str::<toml::Value>(DW_COMMON).expect("通用模板应为合法 TOML");
        toml::from_str::<toml::Value>(DW_SPECIFIC).expect("dw 特有模板应为合法 TOML");

        let dir = TempDir::new().unwrap();
        run(&InitKind::Dw(output_args_in(&dir, false))).unwrap();
        let loaded =
            crate::config::load_config_from(&dir.path().join("config.toml")).expect("应加载成功");
        match loaded.specific {
            crate::config::LoadedSpecific::Dw(cluster) => {
                assert_eq!(cluster.nodes.len(), 2);
            }
            _ => panic!("应解析为 Dw 集群配置"),
        }
    }
}
