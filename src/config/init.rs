use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::cli::{InitKind, InitOutputArgs};

pub fn run(kind: &InitKind) -> Result<()> {
    match kind {
        InitKind::Standalone(args) => {
            let dir = output_dir(args);
            write_template(&dir.join("config.toml"), args.force, STANDALONE_COMMON)?;
            write_template(&dir.join("standalone.toml"), args.force, STANDALONE_SPECIFIC)?;
            println!("已生成单机配置模板:");
            println!("  config.toml      — 通用配置（type、安装包路径等）");
            println!("  standalone.toml  — 单机特有配置（端口、路径、字符集等）");
            println!("编辑后使用: dm-installer install");
        }
        InitKind::Dw(args) => {
            let dir = output_dir(args);
            write_template(&dir.join("config.toml"), args.force, DW_COMMON)?;
            write_template(&dir.join("dw.toml"), args.force, DW_SPECIFIC)?;
            println!("已生成主备集群配置模板:");
            println!("  config.toml  — 通用配置（type、安装包路径等）");
            println!("  dw.toml      — 主备特有配置（节点、OGUID 等）");
            println!("编辑后使用: dm-installer install");
        }
        InitKind::Rws(args) => {
            let dir = output_dir(args);
            write_template(&dir.join("config.toml"), args.force, RWS_COMMON)?;
            write_template(&dir.join("rws.toml"), args.force, RWS_SPECIFIC)?;
            println!("已生成读写分离集群配置模板:");
            println!("  config.toml  — 通用配置（type、安装包路径等）");
            println!("  rws.toml     — 读写分离特有配置（节点、OGUID 等）");
            println!("编辑后使用: dm-installer install");
        }
        InitKind::Dsc(args) => {
            let dir = output_dir(args);
            write_template(&dir.join("config.toml"), args.force, DSC_COMMON)?;
            write_template(&dir.join("dsc.toml"), args.force, DSC_SPECIFIC)?;
            println!("已生成 DSC 共享存储集群配置模板:");
            println!("  config.toml  — 通用配置（type、安装包路径等）");
            println!("  dsc.toml     — DSC 特有配置（节点、OGUID、共享块设备等）");
            println!("编辑后使用: dm-installer install");
        }
    }
    Ok(())
}

/// 返回输出目录：用户指定了 --output 时将其作为目录，否则用当前目录。
fn output_dir(args: &InitOutputArgs) -> PathBuf {
    args.output.clone().unwrap_or_else(|| PathBuf::from("."))
}

fn write_template(path: &Path, force: bool, content: &str) -> Result<()> {
    if path.exists() && !force {
        bail!("文件已存在: {}；使用 --force 强制覆盖", path.display());
    }
    std::fs::write(path, content)
        .map_err(|e| anyhow::anyhow!("无法写入配置文件 {}: {}", path.display(), e))
}

// ─── 模板内容 ──────────────────────────────────────────────────────────────

const STANDALONE_COMMON: &str = r#"# 达梦数据库单机安装 — 通用配置
# 使用方式: dm-installer install

type = "standalone"

# ─── 安装包来源（三选一，都不填则自动检测下载）────────────────
# 本地文件路径
# installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"
# 自定义下载链接
# installer_url = "https://download.example.com/dm8.zip"

# ─── 日志配置 ────────────────────────────────────────────────
[logging]
# 日志级别：trace / debug / info / warn / error
level = "info"
# 日志文件路径（不填则只输出到终端）
# file = "/var/log/dm-installer/install.log"
# 回滚策略：never / daily / hourly（file 有值时生效）
# rotation = "daily"
# 最多保留的历史日志文件数，0 = 不自动删除（rotation != never 时生效）
# max_files = 7
"#;

const STANDALONE_SPECIFIC: &str = r#"# 达梦数据库单机安装 — 特有配置（standalone.toml）
# 注意：SYSDBA / SYSAUDITOR 密码在安装时由终端提示输入，不写入此文件

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
# 单机模式默认开启本地归档（ARCH_INI=1），以下参数均可省略走默认值。
[archive]
# arch_path = "/home/dmdba/dmdbms/data/arch"  # 不填则默认为 data_path/arch
file_size   = 128   # 单归档文件大小（MB）
space_limit = 0     # 归档空间上限（MB），0 = 无限
hang_flag   = false # 归档失败时是否挂起数据库（单机建议 false）
compressed  = false # 是否压缩归档文件

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

const DW_COMMON: &str = r#"# 达梦数据库主备集群 — 通用配置
# 使用方式: dm-installer install

type = "dw"

# ─── 安装包来源（二选一，集群必须指定）──────────────────────
# 本地文件路径（控制机准备好后推送到各节点）
installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"
# 自定义下载链接（控制机下载后推送到各节点）
# installer_url = "https://download.example.com/dm8.zip"

# ─── 日志配置 ────────────────────────────────────────────────
[logging]
level = "info"
# file = "/var/log/dm-installer/install.log"
# rotation = "daily"
# max_files = 7
"#;

const RWS_COMMON: &str = r#"# 达梦数据库读写分离集群 — 通用配置
# 使用方式: dm-installer install

type = "rws"

installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"
# installer_url = "https://download.example.com/dm8.zip"

[logging]
level = "info"
# file = "/var/log/dm-installer/install.log"
# rotation = "daily"
# max_files = 7
"#;

const DSC_COMMON: &str = r#"# 达梦数据库 DSC 共享存储集群 — 通用配置
# 使用方式: dm-installer install

type = "dsc"

installer_package = "/path/to/dm8_setup_rh7_64_ent_8.1.3.100.iso"
# installer_url = "https://download.example.com/dm8.zip"

[logging]
level = "info"
# file = "/var/log/dm-installer/install.log"
# rotation = "daily"
# max_files = 7
"#;

const DW_SPECIFIC: &str = r#"# 达梦数据库主备集群 — 特有配置（dw.toml）

# 守护系统全局唯一标识，主备节点必须相同，范围 0-2147483647
oguid = 453331

# ─── 主节点 ─────────────────────────────────────────────────
[[nodes]]
role          = "primary"
host          = "192.168.1.10"
instance_name = "DMSVR01"
# mal_port     = 5237   # MAL 通信端口（默认 5237）
# dw_port      = 5238   # 守护进程端口（默认 5238）
# inst_dw_port = 5239   # 实例守护端口（默认 5239）

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
# password    = "your_password"

# ─── 备节点 ─────────────────────────────────────────────────
[[nodes]]
role          = "standby"
host          = "192.168.1.11"
instance_name = "DMSVR02"
# mal_port     = 5237
# dw_port      = 5238
# inst_dw_port = 5239

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
# password    = "your_password"

# ─── dminit 初始化参数（集群级统一，各节点共用） ──────────────
[dminit]
install_path   = "/opt/dmdbms"
data_path      = "/opt/dmdbms/data"
port           = 5236
page_size      = 8
charset        = 0       # 0=GB18030  1=UTF-8  2=EUC-KR
case_sensitive = true
extent_size    = 16      # 16 或 32

# ─── dm.ini 集群追加参数 ─────────────────────────────────────
[dm_ini]
enable_offline_ts = 2  # 集群模式离线表空间行为（2=集群推荐值）

# ─── dmarch.ini 归档参数（集群级统一，各节点保持一致） ──────────
[archive]
arch_path    = "/opt/dmdbms/arch"  # 各节点本地归档目录
file_size    = 128                 # 单归档文件大小（MB）
space_limit  = 0                   # 归档空间上限（MB），0 = 无限
hang_flag    = true                # 归档失败时是否挂起数据库（保数据安全）
compressed   = false               # 是否压缩归档文件

# ─── dmmal.ini MAL 链路参数 ──────────────────────────────────
[mal]
check_interval     = 5    # MAL 心跳检测间隔（秒）
conn_fail_interval = 5    # 连接失败重试间隔（秒）
buf_size           = 100  # 单实例发送缓冲区大小（MB）
sys_buf_size       = 512  # 系统级总发送缓冲区大小（MB）
compress_level     = 0    # 数据压缩级别（0=不压缩）

# ─── dmwatcher.ini 守护进程参数 ──────────────────────────────
[watcher]
dw_mode              = "AUTO"  # AUTO（自动故障切换）或 MANUAL（手动）
dw_error_time        = 10      # 守护错误判定时间（秒）
inst_recover_time    = 60      # 实例恢复等待时间（秒）
inst_error_time      = 10      # 实例错误判定时间（秒）
inst_auto_restart    = 1       # 故障后自动重启实例（1=是，0=否）
rlog_send_threshold  = 0       # redo 日志发送阈值（秒），0=不限制
rlog_apply_threshold = 0       # redo 日志应用阈值（秒），0=不限制
# inst_startup_cmd = "/opt/dmdbms/bin/dmserver"  # 自定义启动命令（默认 install_path/bin/dmserver）

# ─── SQL 日志（sqllog.ini）───────────────────────────────────
# enabled = true 时，数据库 open 后通过 disql 调用 SP_SET_PARA_VALUE /
# SP_SET_SQLLOG_PARA_VALUE 写入参数，不直接生成 sqllog.ini 文件。
[sqllog]
enabled       = false  # 是否启用 SQL 日志
file_size     = 64     # 单日志文件大小上限（MB）
file_num      = 128    # 保留的历史文件数
min_exec_time = 0      # 最小执行时间阈值（ms），0 = 记录全部 SQL
"#;

const RWS_SPECIFIC: &str = r#"# 达梦数据库读写分离集群 — 特有配置（rws.toml）

oguid = 453331

# ─── 主节点（处理写入） ─────────────────────────────────────
[[nodes]]
role          = "primary"
host          = "192.168.1.10"
instance_name = "DMSVR01"
# mal_port     = 5237
# dw_port      = 5238
# inst_dw_port = 5239

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
# password    = "your_password"

# ─── 备节点（承担只读查询） ─────────────────────────────────
[[nodes]]
role          = "standby"
read_only     = true
host          = "192.168.1.11"
instance_name = "DMSVR02"
# mal_port     = 5237
# dw_port      = 5238
# inst_dw_port = 5239

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
# password    = "your_password"

# ─── dminit 初始化参数（集群级统一，各节点共用） ──────────────
[dminit]
install_path   = "/opt/dmdbms"
data_path      = "/opt/dmdbms/data"
port           = 5236
page_size      = 8
charset        = 0
case_sensitive = true
extent_size    = 16

[dm_ini]
enable_offline_ts = 2

[archive]
arch_path    = "/opt/dmdbms/arch"
file_size    = 128
space_limit  = 0
hang_flag    = true
compressed   = false

[mal]
check_interval     = 5
conn_fail_interval = 5
buf_size           = 100
sys_buf_size       = 512
compress_level     = 0

[watcher]
dw_mode              = "AUTO"
dw_error_time        = 10
inst_recover_time    = 60
inst_error_time      = 10
inst_auto_restart    = 1
rlog_send_threshold  = 0
rlog_apply_threshold = 0
# inst_startup_cmd = "/opt/dmdbms/bin/dmserver"

[sqllog]
enabled       = false
file_size     = 64
file_num      = 128
min_exec_time = 0
"#;

const DSC_SPECIFIC: &str = r#"# 达梦数据库 DSC 共享存储集群 — 特有配置（dsc.toml）

# 守护系统全局唯一标识，所有节点必须相同，范围 0-2147483647
oguid = 453331

# ─── 共享块设备路径（四个设备，路径必须互不相同且非空） ────────
[dsc_storage]
dcr_disk  = "/dev/raw/raw1"   # DCR 控制文件磁盘
vote_disk = "/dev/raw/raw2"   # 投票磁盘
log_disk  = "/dev/raw/raw3"   # 日志磁盘（DMLOG ASM 磁盘组）
data_disk = "/dev/raw/raw4"   # 数据磁盘（DMDATA ASM 磁盘组）

# ─── 节点 1（负责 dminit 初始化共享实例） ──────────────────
[[nodes]]
role          = "primary"
host          = "192.168.1.10"
instance_name = "DMSVR01"

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
# password    = "your_password"

# ─── 节点 2 ─────────────────────────────────────────────────
[[nodes]]
role          = "standby"
host          = "192.168.1.11"
instance_name = "DMSVR02"

[nodes.ssh]
user          = "root"
identity_file = "~/.ssh/id_rsa"
# password    = "your_password"

# ─── dminit 初始化参数（集群级统一） ─────────────────────────
[dminit]
install_path   = "/opt/dmdbms"
data_path      = "/dmdata"
port           = 5236
page_size      = 8
charset        = 0       # 0=GB18030  1=UTF-8  2=EUC-KR
case_sensitive = true
extent_size    = 16      # 16 或 32
"#;


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn output_args_in(dir: &TempDir, force: bool) -> InitOutputArgs {
        InitOutputArgs { output: Some(dir.path().to_path_buf()), force }
    }

    #[test]
    fn test_standalone_creates_two_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        assert!(dir.path().join("config.toml").exists(), "应生成 config.toml");
        assert!(dir.path().join("standalone.toml").exists(), "应生成 standalone.toml");
    }

    #[test]
    fn test_standalone_common_has_type_field() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(content.contains("type = \"standalone\""), "通用配置应含 type = \"standalone\"");
    }

    #[test]
    fn test_standalone_specific_has_install_fields() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("standalone.toml")).unwrap();
        assert!(content.contains("install_path"), "特有配置应含 install_path");
        assert!(content.contains("port = 5236"), "特有配置应含默认端口");
    }

    #[test]
    fn test_standalone_templates_are_valid_toml() {
        toml::from_str::<toml::Value>(STANDALONE_COMMON).expect("通用模板应为合法 TOML");
        toml::from_str::<toml::Value>(STANDALONE_SPECIFIC).expect("单机特有模板应为合法 TOML");
    }

    #[test]
    fn test_dw_creates_two_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Dw(output_args_in(&dir, false))).unwrap();
        assert!(dir.path().join("config.toml").exists(), "应生成 config.toml");
        assert!(dir.path().join("dw.toml").exists(), "应生成 dw.toml");
    }

    #[test]
    fn test_dw_common_has_correct_type() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Dw(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(content.contains("type = \"dw\""), "通用配置应含正确的 type");
    }

    #[test]
    fn test_dw_specific_has_nodes() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Dw(output_args_in(&dir, false))).unwrap();
        let content = std::fs::read_to_string(dir.path().join("dw.toml")).unwrap();
        assert!(content.contains("\"primary\""), "特有配置应含 primary 节点");
        assert!(content.contains("\"standby\""), "特有配置应含 standby 节点");
    }

    #[test]
    fn test_rws_creates_two_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Rws(output_args_in(&dir, false))).unwrap();
        assert!(dir.path().join("config.toml").exists());
        assert!(dir.path().join("rws.toml").exists());
        let content = std::fs::read_to_string(dir.path().join("rws.toml")).unwrap();
        assert!(content.contains("read_only"), "应含 read_only 字段");
        assert!(content.contains("true"), "read_only 应为 true");
    }

    #[test]
    fn test_dsc_creates_two_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Dsc(output_args_in(&dir, false))).unwrap();
        assert!(dir.path().join("config.toml").exists());
        assert!(dir.path().join("dsc.toml").exists());
        let content = std::fs::read_to_string(dir.path().join("dsc.toml")).unwrap();
        assert!(content.contains("[dsc_storage]"), "应含 [dsc_storage] 块");
        assert!(content.contains("dcr_disk"), "应含 dcr_disk 字段");
    }

    #[test]
    fn test_refuses_to_overwrite_without_force() {
        let dir = TempDir::new().unwrap();
        // 先生成一次
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        // 再生成应该因为 config.toml 已存在而报错
        let err = run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap_err();
        assert!(format!("{err}").contains("文件已存在"));
    }

    #[test]
    fn test_force_overwrites_existing_files() {
        let dir = TempDir::new().unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, false))).unwrap();
        run(&InitKind::Standalone(output_args_in(&dir, true))).unwrap();
        assert!(dir.path().join("standalone.toml").exists());
    }

    #[test]
    fn test_all_templates_are_valid_toml() {
        for (name, tmpl) in [
            ("STANDALONE_COMMON", STANDALONE_COMMON),
            ("STANDALONE_SPECIFIC", STANDALONE_SPECIFIC),
            ("DW_COMMON", DW_COMMON),
            ("DW_SPECIFIC", DW_SPECIFIC),
            ("RWS_SPECIFIC", RWS_SPECIFIC),
            ("DSC_SPECIFIC", DSC_SPECIFIC),
        ] {
            toml::from_str::<toml::Value>(tmpl)
                .unwrap_or_else(|e| panic!("{name} 应为合法 TOML: {e}"));
        }
    }
}
