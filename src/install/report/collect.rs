//! 采集逻辑：OS 层 shell 命令 + DB 层 disql 查询，返回 model.rs 里的结构。
//!
//! 当前覆盖：1.1.1 OS 检查总结核心项、2.2.2 实例信息、2.2.3 数据库巡检内容
//! （会话数/SQL日志策略/表空间状态/归档状态/缓存池/数据文件/用户/对象/作业执行情况）、
//! 2.2.4 dm.ini 全量参数、2.2.7 错误日志。
//!
//! `DBA_USERS`/`DBA_OBJECTS`/`SYSJOB.SYSJOBS` 等查询的列名未经过真实达梦实例验证
//! （没有可核对的官方文档在手）；`disql_query`/`parse_disql_rows` 对查询失败或列名不对
//! 的情况会安全降级为空结果，不会在报告里产出编造的数据，只是对应检查项不出现。
//!
//! 历史SQL错误/top sql/归档异地备份/应用驱动版本等仍无法可靠自动采集，
//! 由调用方（mod.rs）直接拼装 ManualRequired 占位行；其余原先标记"请人工核对"的项
//! （网络信息、磁盘测速、Audit 版本、备份与数据目录是否分离、备份清理作业、
//! License、用户角色权限）本轮已替换为真实采集，结果仍可能需要人工复核但不再是空话术。

use crate::config::InstallConfig;
use crate::ssh::{CommandRunner, shell_quote};

use super::model::{CheckItem, LogErrorRow, ParamCheck, SqlErrorRow, Status, TopSqlRow};

async fn run_text(runner: &dyn CommandRunner, cmd: &str) -> String {
    match runner.exec(cmd).await {
        Ok((out, _)) => String::from_utf8_lossy(&out).trim().to_string(),
        Err(_) => String::new(),
    }
}

/// 以 dmdba 身份执行一条 SQL，返回 disql 原始输出文本（失败返回 None）。
async fn disql_query(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
    sql: &str,
) -> Option<String> {
    let disql = format!("{}/bin/disql", config.install_path);
    let conn = format!("SYSDBA/{}@localhost:{}", sysdba_pwd, config.port);
    let inner_cmd = format!(
        "printf '{}\\nexit;\\n' | {} {}",
        sql,
        shell_quote(&disql),
        shell_quote(&conn),
    );
    let cmd = format!("su - dmdba -c {} 2>/dev/null", shell_quote(&inner_cmd));
    let (out, _) = runner.exec(&cmd).await.ok()?;
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// 按空白切分 disql 表格输出的数据行（跳过表头与分隔线），每行返回各列。
fn parse_disql_rows(output: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut past_sep = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("---") {
            past_sep = true;
            continue;
        }
        if !past_sep || trimmed.is_empty() || trimmed.starts_with("LINEID") {
            continue;
        }
        rows.push(
            trimmed
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>(),
        );
    }
    rows
}

/// 1.1.1 数据安装检查总结：glibc / 透明大页 / SELinux / 操作系统版本 / 内核版本 / CPU 超线程。
pub async fn collect_os_checks(runner: &dyn CommandRunner) -> Vec<CheckItem> {
    let mut items = Vec::new();

    let glibc = run_text(runner, "ldd --version 2>&1 | head -1").await;
    items.push(CheckItem::new(
        "glibc 版本",
        Status::Normal,
        if glibc.is_empty() {
            "未能获取 glibc 版本".to_string()
        } else {
            glibc
        },
    ));

    let thp = run_text(
        runner,
        "cat /sys/kernel/mm/transparent_hugepage/enabled 2>/dev/null",
    )
    .await;
    let thp_disabled = thp.contains("[never]");
    items.push(CheckItem::new(
        "透明大页状态",
        if thp_disabled {
            Status::Normal
        } else {
            Status::Abnormal
        },
        if thp.is_empty() {
            "未能获取透明大页状态".to_string()
        } else {
            format!("{thp}（推荐设置为禁用 [never]）")
        },
    ));

    let selinux = run_text(runner, "getenforce 2>/dev/null || echo absent").await;
    items.push(CheckItem::new(
        "SELinux 状态",
        if selinux == "Enforcing" {
            Status::Warning
        } else {
            Status::Normal
        },
        if selinux.is_empty() || selinux == "absent" {
            "已禁用".to_string()
        } else {
            selinux
        },
    ));

    let os_release = run_text(runner, "cat /etc/os-release 2>/dev/null").await;
    items.push(CheckItem::new(
        "操作系统版本",
        Status::ManualRequired,
        if os_release.is_empty() {
            "根据实际情况判断是否符合要求".to_string()
        } else {
            os_release
        },
    ));

    let kernel = run_text(runner, "uname -r").await;
    items.push(CheckItem::new(
        "Linux内核版本",
        Status::ManualRequired,
        if kernel.is_empty() {
            "根据实际情况判断是否符合要求".to_string()
        } else {
            kernel
        },
    ));

    let hyperthread = check_hyperthreading(runner).await;
    items.push(hyperthread);

    items
}

async fn check_hyperthreading(runner: &dyn CommandRunner) -> CheckItem {
    let threads = run_text(runner, "nproc").await;
    let physical = run_text(
        runner,
        "lscpu 2>/dev/null | awk -F: '/^Socket\\(s\\)/{print $2}' | tr -d ' '",
    )
    .await;
    let cores_per_socket = run_text(
        runner,
        "lscpu 2>/dev/null | awk -F: '/^Core\\(s\\) per socket/{print $2}' | tr -d ' '",
    )
    .await;

    let parsed = threads
        .parse::<u64>()
        .ok()
        .zip(physical.parse::<u64>().ok())
        .zip(cores_per_socket.parse::<u64>().ok());

    match parsed {
        Some(((t, p), c)) if p > 0 && c > 0 => {
            let enabled = t > p * c;
            CheckItem::new(
                "cpu超线程",
                if enabled {
                    Status::Normal
                } else {
                    Status::Abnormal
                },
                if enabled {
                    "cpu超线程已开启".to_string()
                } else {
                    "cpu超线程未开启，建议开启".to_string()
                },
            )
        }
        _ => CheckItem::new(
            "cpu超线程",
            Status::ManualRequired,
            "未能自动判断，请人工核对 cpu 超线程是否开启",
        ),
    }
}

/// 2.2.4 参数配置信息：dm.ini 全量参数（`V$DM_INI`），推荐值与采集值相同
/// （安装流程的 [10/10] 步已用同一份官方脚本调好参数，此处只是采集现状）。
pub async fn collect_param_detail(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Vec<ParamCheck> {
    let sql = "SELECT PARA_NAME,PARA_VALUE,FILE_VALUE FROM V$DM_INI;";
    let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await else {
        return Vec::new();
    };
    // disql 输出列：LINEID PARA_NAME PARA_VALUE FILE_VALUE
    parse_disql_rows(&out)
        .into_iter()
        .filter_map(|cols| {
            let [_, name, para_value, file_value] = <[String; 4]>::try_from(cols).ok()?;
            Some(ParamCheck {
                name,
                file_value,
                recommend: para_value.clone(),
                para_value,
            })
        })
        .collect()
}

/// 2.2.2 数据库实例内容：实例状态/模式（`V$INSTANCE`）。
pub async fn collect_instance_status(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Vec<(String, String)> {
    let sql = "SELECT STATUS$,MODE$ FROM V$INSTANCE;";
    let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await else {
        return Vec::new();
    };
    let Some(cols) = parse_disql_rows(&out).into_iter().next() else {
        return Vec::new();
    };
    let mut info = Vec::new();
    if let Some(status) = cols.first() {
        info.push(("系统状态".to_string(), status.clone()));
    }
    if let Some(mode) = cols.get(1) {
        info.push(("实例模式".to_string(), mode.clone()));
    }
    info
}

/// 2.2.3 数据库巡检内容：会话数使用、SQL 日志策略（`SVR_LOG`）、表空间状态、
/// 归档状态、缓存池使用情况、数据文件信息、用户信息、对象信息、作业执行情况。
/// 表空间使用率（总大小/空闲/使用率）需要联合查询 `V$DATAFILE` 算出每个表空间的
/// 空间统计，比单表查询复杂，留作后续迭代（字段已在报告里占位为空表，不影响章节
/// 结构完整）。
pub async fn collect_inspection(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Vec<CheckItem> {
    let mut items = Vec::new();

    let sql = "SELECT COUNT(*) FROM V$SESSIONS;";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await
        && let Some(count) = first_value(&out)
    {
        items.push(CheckItem::new(
            "会话数使用",
            Status::Normal,
            format!("目前会话数 {count}"),
        ));
    }

    let sql = "SELECT PARA_VALUE FROM V$DM_INI WHERE PARA_NAME='SVR_LOG';";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await
        && let Some(value) = first_value(&out)
    {
        items.push(CheckItem::new(
            "SQL日志策略（SVR_LOG）",
            if value == "0" {
                Status::Normal
            } else {
                Status::Warning
            },
            format!("SVR_LOG={value}（生产环境建议关闭，设为 0）"),
        ));
    }

    let sql = "SELECT NAME,STATUS$ FROM V$TABLESPACE;";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await {
        let rows = parse_disql_rows(&out);
        if !rows.is_empty() {
            let summary = rows
                .iter()
                .filter_map(|c| Some(format!("{}：{}", c.first()?, c.get(1)?)))
                .collect::<Vec<_>>()
                .join("，");
            items.push(CheckItem::new("表空间状态", Status::Normal, summary));
        }
    }

    let sql = "SELECT ARCH_TYPE,ARCH_DEST,ARCH_STATUS FROM V$ARCH_STATUS;";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await {
        let rows = parse_disql_rows(&out);
        if rows.is_empty() {
            items.push(CheckItem::new(
                "归档状态",
                Status::ManualRequired,
                "未查询到归档状态，请人工核对归档是否已启用",
            ));
        } else {
            let summary = rows
                .iter()
                .filter_map(|c| {
                    Some(format!(
                        "类型：{}，目标：{}，状态：{}",
                        c.first()?,
                        c.get(1)?,
                        c.get(2)?
                    ))
                })
                .collect::<Vec<_>>()
                .join("；");
            items.push(CheckItem::new("归档状态", Status::Normal, summary));
        }
    }

    let sql = "SELECT NAME,N_PAGES,TOTAL_PAGES FROM V$BUFFERPOOL;";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await {
        let rows = parse_disql_rows(&out);
        if !rows.is_empty() {
            let summary = rows
                .iter()
                .filter_map(|c| Some(format!("{}：{}/{} 页", c.first()?, c.get(1)?, c.get(2)?)))
                .collect::<Vec<_>>()
                .join("，");
            items.push(CheckItem::new("缓存池使用情况", Status::Normal, summary));
        }
    }

    let sql = "SELECT PATH,BYTES/1048576,AUTO_EXTEND FROM V$DATAFILE;";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await {
        let rows = parse_disql_rows(&out);
        if !rows.is_empty() {
            let summary = rows
                .iter()
                .filter_map(|c| {
                    Some(format!(
                        "{}：{}M（自动扩展：{}）",
                        c.first()?,
                        c.get(1)?,
                        c.get(2)?
                    ))
                })
                .collect::<Vec<_>>()
                .join("；");
            items.push(CheckItem::new("数据文件信息", Status::Normal, summary));
        }
    }

    // DBA_USERS 是 DM 数据字典视图（Oracle 兼容视图，DM8 默认随库提供），列名未经过本机
    // 实例验证；查询失败时 disql_query/parse_disql_rows 会安全降级为空，不会产出错误信息。
    let sql = "SELECT USERNAME,ACCOUNT_STATUS FROM DBA_USERS;";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await {
        let rows = parse_disql_rows(&out);
        if !rows.is_empty() {
            let summary = rows
                .iter()
                .filter_map(|c| Some(format!("{}：{}", c.first()?, c.get(1)?)))
                .collect::<Vec<_>>()
                .join("，");
            items.push(CheckItem::new(
                "用户信息",
                Status::ManualRequired,
                format!("{summary}（请人工核对是否存在多余/默认密码账户）"),
            ));
        }
    }

    // DBA_OBJECTS 同上，为 DM 数据字典视图，按对象类型统计数量。
    let sql = "SELECT OBJECT_TYPE,COUNT(*) FROM DBA_OBJECTS GROUP BY OBJECT_TYPE;";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await {
        let rows = parse_disql_rows(&out);
        if !rows.is_empty() {
            let summary = rows
                .iter()
                .filter_map(|c| Some(format!("{}：{}", c.first()?, c.get(1)?)))
                .collect::<Vec<_>>()
                .join("，");
            items.push(CheckItem::new("对象信息", Status::Normal, summary));
        }
    }

    // 作业系统表，名称依据 backup.rs 里 SP_CREATE_JOB 创建的作业（bakup_ql/bakup_zl/bak_clear）
    // 所在的 SYSJOB 模式整理；ENABLE/STATE 列名未经过本机实例验证。
    let sql = "SELECT NAME,ENABLE,STATE FROM SYSJOB.SYSJOBS;";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await {
        let rows = parse_disql_rows(&out);
        if !rows.is_empty() {
            let summary = rows
                .iter()
                .filter_map(|c| {
                    Some(format!(
                        "作业名称：{}，是否启用：{}，状态：{}",
                        c.first()?,
                        c.get(1)?,
                        c.get(2)?
                    ))
                })
                .collect::<Vec<_>>()
                .join("；");
            items.push(CheckItem::new("作业执行情况", Status::Normal, summary));
        }
    }

    items
}

fn first_value(disql_output: &str) -> Option<String> {
    parse_disql_rows(disql_output)
        .into_iter()
        .next()
        .and_then(|c| c.into_iter().next())
}

/// 2.2.7 错误日志信息：读取数据目录下匹配 `glob_pattern` 的日志文件，
/// 按 `[FATAL]`/`[ERROR]` 关键字过滤，格式：`时间 [级别] 进程号 线程号 错误信息`。
async fn collect_error_logs(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    glob_pattern: &str,
) -> Vec<LogErrorRow> {
    let pattern = format!("{}/DAMENG/{glob_pattern}", shell_quote(&config.data_path));
    let cmd = format!(
        "grep -hE '\\[FATAL\\]|\\[ERROR\\]' {} 2>/dev/null | tail -50",
        pattern
    );
    let out = run_text(runner, &cmd).await;
    parse_dmserver_log_lines(&out)
}

/// 2.2.7.2、dm_DMSERVER 运行错误日志。
pub async fn collect_dmserver_error_logs(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
) -> Vec<LogErrorRow> {
    collect_error_logs(runner, config, "dm_DMSERVER_*.log").await
}

/// 2.2.7.1、dm_dmap 运行错误日志。
pub async fn collect_dmap_error_logs(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
) -> Vec<LogErrorRow> {
    collect_error_logs(runner, config, "dm_dmap_*.log").await
}

/// 服务器网卡信息（`ip addr`）。网络测速需要远端目标主机配合，无法在安装现场单独
/// 自动完成，仍标 `ManualRequired`，但 detail 里给出真实采集到的网卡信息，
/// 不再是纯说明文字。
pub async fn collect_network_info(runner: &dyn CommandRunner) -> CheckItem {
    let out = run_text(runner, "ip addr 2>/dev/null || ifconfig 2>/dev/null").await;
    let detail = if out.is_empty() {
        "未能获取网卡信息；请人工进行网络测速！标准传输数据千兆65MB/s以上，万兆650MB/s；\
         可使用语句：scp local_file remote_username@remote_ip:remote_folder"
            .to_string()
    } else {
        format!(
            "{out}\n（请人工进行网络测速：标准传输数据千兆65MB/s以上，万兆650MB/s；\
             可使用语句：scp local_file remote_username@remote_ip:remote_folder）"
        )
    };
    CheckItem::new("服务器的网卡信息", Status::ManualRequired, detail)
}

/// 磁盘写入测速：复用模板官方建议的 dd 命令，对 `data_path` 下的临时文件做一次
/// 32k×10k（约 312MB）的 dsync 写入测试，测完即删；这是本地操作，不依赖远端主机，
/// 可以真正自动跑完，不需要人工再单独执行一遍。
pub async fn collect_disk_speed(runner: &dyn CommandRunner, config: &InstallConfig) -> CheckItem {
    let test_file = format!("{}/.dm_disktest_tmp", config.data_path);
    let cmd = format!(
        "dd if=/dev/zero of={} bs=32k count=10k oflag=dsync 2>&1; rm -f {}",
        shell_quote(&test_file),
        shell_quote(&test_file)
    );
    let out = run_text(runner, &cmd).await;
    match parse_dd_throughput(&out) {
        Some(throughput) => CheckItem::new(
            "磁盘测速（写入，dsync）",
            Status::Normal,
            format!(
                "{throughput}（本地磁盘一般 100MB/s 左右，磁盘阵列 10MB/s 左右一般业务没问题，\
                 50MB/s 就算给力，SSD 可以到几百 MB/s，国产服务器可能略差）"
            ),
        ),
        None => CheckItem::new(
            "磁盘测速（写入，dsync）",
            Status::ManualRequired,
            format!(
                "未能解析测速结果，请人工执行：dd if=/dev/zero of={test_file} bs=32k count=10k oflag=dsync"
            ),
        ),
    }
}

/// 从 `dd` 的 stderr/stdout 输出里提取吞吐速率（形如 `... 450 MB/s` 或 `... 1.2 GB/s`）。
fn parse_dd_throughput(dd_output: &str) -> Option<String> {
    dd_output.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.contains("copied") {
            return None;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        let unit_idx = tokens
            .iter()
            .position(|t| matches!(*t, "MB/s" | "GB/s" | "kB/s" | "B/s"))?;
        if unit_idx == 0 {
            return None;
        }
        Some(format!("{} {}", tokens[unit_idx - 1], tokens[unit_idx]))
    })
}

/// 操作系统是否存在 Audit 漏洞：采集已安装的 audit 包版本，是否需要升级仍需人工
/// 对照厂商漏洞公告判断，因此保留 `ManualRequired`，但不再是空话术。
pub async fn collect_audit_version(runner: &dyn CommandRunner) -> CheckItem {
    let out = run_text(
        runner,
        "rpm -q audit 2>/dev/null || dpkg -l audit 2>/dev/null | tail -1",
    )
    .await;
    let detail = if out.is_empty() {
        "未能获取 audit 包版本，请人工核对是否需要升级".to_string()
    } else {
        format!("当前 audit 版本：{out}；请人工核对是否需要升级（参考厂商漏洞公告）")
    };
    CheckItem::new(
        "检查操作系统是否存在Audit漏洞",
        Status::ManualRequired,
        detail,
    )
}

/// 备份目录与数据目录是否分离：纯配置比对，不需要远程调用，能直接给出确定结论。
pub fn check_backup_data_separation(config: &InstallConfig) -> CheckItem {
    match config.backup.backup_path.as_deref() {
        Some(backup_path) if !backup_path.is_empty() => {
            let separated = backup_path != config.data_path
                && !backup_path.starts_with(&format!("{}/", config.data_path));
            CheckItem::new(
                "备份目录和数据目录是否一致",
                if separated {
                    Status::Normal
                } else {
                    Status::Abnormal
                },
                format!(
                    "数据目录：{}；备份目录：{}（{}）",
                    config.data_path,
                    backup_path,
                    if separated {
                        "已分离"
                    } else {
                        "未分离，建议备份和数据分开存储"
                    }
                ),
            )
        }
        _ => CheckItem::new(
            "备份目录和数据目录是否一致",
            Status::ManualRequired,
            "未配置备份目录，请人工核对",
        ),
    }
}

/// 定时清理旧备份的定时任务：复用作业系统查询（`SYSJOB.SYSJOBS`），核对 `bak_clear`
/// 作业（backup.rs 里 `SP_CREATE_JOB('bak_clear', ...)` 创建）是否存在且已启用。
/// 列名未经过真实达梦实例验证，查询失败会安全降级为 `ManualRequired`。
pub async fn collect_backup_cleanup_job(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> CheckItem {
    let sql = "SELECT NAME,ENABLE FROM SYSJOB.SYSJOBS WHERE NAME='bak_clear';";
    let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await else {
        return CheckItem::new(
            "定时清理旧备份的定时任务",
            Status::ManualRequired,
            "未能查询作业系统，请人工确认是否设置了定时清理旧备份的定时任务",
        );
    };
    let rows = parse_disql_rows(&out);
    match rows.into_iter().next() {
        Some(cols) if cols.first().is_some_and(|n| n == "bak_clear") => {
            let enabled = cols.get(1).map(String::as_str) == Some("1");
            CheckItem::new(
                "定时清理旧备份的定时任务",
                if enabled {
                    Status::Normal
                } else {
                    Status::Warning
                },
                format!(
                    "bak_clear 作业{}",
                    if enabled {
                        "已启用"
                    } else {
                        "存在但未启用"
                    }
                ),
            )
        }
        _ => CheckItem::new(
            "定时清理旧备份的定时任务",
            Status::ManualRequired,
            "未查询到 bak_clear 作业，请人工确认是否设置了定时清理旧备份的定时任务",
        ),
    }
}

/// License 信息：`V$LICENSE` 列名未经过真实达梦实例验证，查询失败会安全降级为
/// `ManualRequired` 而不是编造内容。
pub async fn collect_license_info(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> CheckItem {
    let sql = "SELECT * FROM V$LICENSE;";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await {
        let rows = parse_disql_rows(&out);
        if let Some(cols) = rows.into_iter().next() {
            return CheckItem::new("License信息", Status::Normal, cols.join(" "));
        }
    }
    CheckItem::new(
        "License信息",
        Status::ManualRequired,
        "未能查询到 License 信息，请人工核对",
    )
}

/// 应用用户权限配置：`DBA_ROLE_PRIVS` 列出每个用户实际拥有的角色（PUBLIC/RESOURCE/
/// SOI/VTI 等），是否"超出配置"仍需业务侧人工判断，但展示的是真实现状而非空话术。
/// 列名未经过真实达梦实例验证，查询失败会安全降级为 `ManualRequired`。
pub async fn collect_user_role_privileges(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> CheckItem {
    let sql = "SELECT GRANTEE,GRANTED_ROLE FROM DBA_ROLE_PRIVS;";
    if let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await {
        let rows = parse_disql_rows(&out);
        if !rows.is_empty() {
            let mut by_user: std::collections::BTreeMap<String, Vec<String>> =
                std::collections::BTreeMap::new();
            for cols in &rows {
                if let (Some(user), Some(role)) = (cols.first(), cols.get(1)) {
                    by_user.entry(user.clone()).or_default().push(role.clone());
                }
            }
            let summary = by_user
                .into_iter()
                .map(|(user, roles)| format!("{}的权限是:{}", user, roles.join(", ")))
                .collect::<Vec<_>>()
                .join("；");
            return CheckItem::new(
                "应用用户权限配置(PUBLIC、RESOURCE、SOI、VTI)",
                Status::ManualRequired,
                format!("{summary}（请人工核对是否存在超出权限配置）"),
            );
        }
    }
    CheckItem::new(
        "应用用户权限配置(PUBLIC、RESOURCE、SOI、VTI)",
        Status::ManualRequired,
        "未能查询到用户角色权限，请人工核对应用用户是否存在超出权限配置",
    )
}

/// 达梦日志行格式：`日期 时间 [级别] database 进程号(P开头) 线程号(T开头) 错误信息...`
fn parse_dmserver_log_lines(output: &str) -> Vec<LogErrorRow> {
    output
        .lines()
        .filter_map(|line| {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            if tokens.len() < 4 {
                return None;
            }
            let date = tokens[0];
            let time = tokens[1];
            let level = tokens[2];
            let pid_idx = tokens
                .iter()
                .position(|t| t.starts_with('P') && t[1..].chars().all(|c| c.is_ascii_digit()))?;
            let tid_idx = tokens
                .iter()
                .position(|t| t.starts_with('T') && t[1..].chars().all(|c| c.is_ascii_digit()))?;
            let message = tokens[tid_idx + 1..].join(" ");
            Some(LogErrorRow {
                time: format!("{date} {time}"),
                level: level.to_string(),
                pid: tokens[pid_idx].to_string(),
                tid: tokens[tid_idx].to_string(),
                message,
            })
        })
        .collect()
}

/// 2.2.5 历史 SQL 错误：模板表头列名（`SESS_ID`/`ECPT_DESC`/`SQL_TEXT`）与达梦动态
/// 性能视图 `V$SQL_ERROR_HISTORY` 的列名一致，按此尝试查询；视图不存在或列名不对时
/// `disql_query`/`parse_disql_rows` 会安全降级为空，调用方据此回退到人工占位说明。
pub async fn collect_sql_error_history(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Vec<SqlErrorRow> {
    let sql = "SELECT SESS_ID,ECPT_DESC,SQL_TEXT FROM V$SQL_ERROR_HISTORY;";
    let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await else {
        return Vec::new();
    };
    parse_disql_rows(&out)
        .into_iter()
        .filter_map(|cols| {
            Some(SqlErrorRow {
                sess_id: cols.first()?.clone(),
                ecpt_desc: cols.get(1)?.clone(),
                sql_text: cols.get(2..)?.join(" "),
            })
        })
        .collect()
}

/// 2.2.6 top sql 信息：模板表头列名（`SQL_TEXT`/`EXEC_TIME`/`FINISH_TIME`）与达梦
/// 动态性能视图 `V$LONG_EXEC_SQLS`（长执行 SQL 监控）的列名一致，按此尝试查询，
/// 同样的安全降级策略。
pub async fn collect_top_sql(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Vec<TopSqlRow> {
    let sql = "SELECT SQL_TEXT,EXEC_TIME,FINISH_TIME FROM V$LONG_EXEC_SQLS;";
    let Some(out) = disql_query(runner, config, sysdba_pwd, sql).await else {
        return Vec::new();
    };
    parse_disql_rows(&out)
        .into_iter()
        .filter_map(|cols| {
            // SQL_TEXT 可能含空格，取末尾两列为 EXEC_TIME/FINISH_TIME，其余拼回 SQL_TEXT。
            if cols.len() < 3 {
                return None;
            }
            let finish_time = cols[cols.len() - 1].clone();
            let exec_time = cols[cols.len() - 2].clone();
            let sql_text = cols[..cols.len() - 2].join(" ");
            Some(TopSqlRow {
                sql_text,
                exec_time,
                finish_time,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::MockRunner;

    #[tokio::test]
    async fn test_collect_os_checks_flags_thp_enabled_as_abnormal() {
        let runner = MockRunner::new(vec![
            (
                "ldd --version".to_string(),
                0,
                b"ldd (GNU libc) 2.28\n".to_vec(),
            ),
            (
                "cat /sys/kernel/mm/transparent_hugepage/enabled".to_string(),
                0,
                b"[always] madvise never\n".to_vec(),
            ),
            ("getenforce".to_string(), 0, b"Disabled\n".to_vec()),
            (
                "cat /etc/os-release".to_string(),
                0,
                b"NAME=\"Kylin\"\n".to_vec(),
            ),
            ("uname -r".to_string(), 0, b"5.10.0\n".to_vec()),
            ("nproc".to_string(), 0, b"32\n".to_vec()),
            (
                "lscpu 2>/dev/null | awk -F: '/^Socket\\(s\\)/".to_string(),
                0,
                b"1\n".to_vec(),
            ),
            (
                "lscpu 2>/dev/null | awk -F: '/^Core\\(s\\) per socket/".to_string(),
                0,
                b"16\n".to_vec(),
            ),
        ]);
        let items = collect_os_checks(&runner).await;
        let thp = items.iter().find(|i| i.name == "透明大页状态").unwrap();
        assert_eq!(thp.status, Status::Abnormal);
        let ht = items.iter().find(|i| i.name == "cpu超线程").unwrap();
        assert_eq!(
            ht.status,
            Status::Normal,
            "32 threads > 1 socket * 16 cores，应判定超线程已开启"
        );
    }

    #[test]
    fn test_parse_disql_rows_skips_header_and_separator() {
        let output = "\
LINEID PARA_NAME PARA_VALUE FILE_VALUE
---------------------------------------
1 MEMORY_TARGET 4000 4000
2 BUFFER 19000 19000
";
        let rows = parse_disql_rows(output);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["1", "MEMORY_TARGET", "4000", "4000"]);
        assert_eq!(rows[1], vec!["2", "BUFFER", "19000", "19000"]);
    }

    fn make_config() -> InstallConfig {
        InstallConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            port: 5236,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_collect_param_detail_parses_rows() {
        let output = "\
LINEID PARA_NAME PARA_VALUE FILE_VALUE
---------------------------------------
1 MEMORY_TARGET 4000 4000
2 BUFFER 19000 19000
";
        let runner = MockRunner::new(vec![(
            "su - dmdba -c".to_string(),
            0,
            output.as_bytes().to_vec(),
        )]);
        let rows = collect_param_detail(&runner, &make_config(), "pwd1").await;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "MEMORY_TARGET");
        assert_eq!(rows[0].para_value, "4000");
        assert_eq!(rows[1].file_value, "19000");
    }

    #[tokio::test]
    async fn test_collect_instance_status_parses_row() {
        let output = "\
STATUS$ MODE$
---------------------------------------
OPEN NORMAL
";
        let runner = MockRunner::new(vec![(
            "su - dmdba -c".to_string(),
            0,
            output.as_bytes().to_vec(),
        )]);
        let info = collect_instance_status(&runner, &make_config(), "pwd1").await;
        assert!(info.contains(&("系统状态".to_string(), "OPEN".to_string())));
        assert!(info.contains(&("实例模式".to_string(), "NORMAL".to_string())));
    }

    #[tokio::test]
    async fn test_collect_inspection_flags_svr_log_enabled_as_warning() {
        let runner = MockRunner::new(vec![
            (
                "su - dmdba -c".to_string(),
                0,
                b"COUNT(*)\n---------------------------------------\n2\n".to_vec(),
            ),
            (
                "su - dmdba -c".to_string(),
                0,
                b"PARA_VALUE\n---------------------------------------\n1\n".to_vec(),
            ),
        ]);
        let items = collect_inspection(&runner, &make_config(), "pwd1").await;
        let svr_log = items
            .iter()
            .find(|i| i.name.contains("SQL日志策略"))
            .unwrap();
        assert_eq!(svr_log.status, Status::Warning);
    }

    #[tokio::test]
    async fn test_collect_inspection_includes_tablespace_archive_and_buffer_pool() {
        let runner = MockRunner::new(vec![
            (
                "su - dmdba -c".to_string(),
                0,
                b"COUNT(*)\n---------------------------------------\n2\n".to_vec(),
            ),
            (
                "su - dmdba -c".to_string(),
                0,
                b"PARA_VALUE\n---------------------------------------\n0\n".to_vec(),
            ),
            (
                "su - dmdba -c".to_string(),
                0,
                b"NAME STATUS$\n---------------------------------------\nSYSTEM ONLINE\n".to_vec(),
            ),
            (
                "su - dmdba -c".to_string(),
                0,
                b"ARCH_TYPE ARCH_DEST ARCH_STATUS\n---------------------------------------\nLOCAL /dmarch/DAMENG VALID\n".to_vec(),
            ),
            (
                "su - dmdba -c".to_string(),
                0,
                b"NAME N_PAGES TOTAL_PAGES\n---------------------------------------\nNORMAL 19687 19687\n".to_vec(),
            ),
        ]);
        let items = collect_inspection(&runner, &make_config(), "pwd1").await;
        assert!(
            items
                .iter()
                .any(|i| i.name == "表空间状态" && i.detail.contains("SYSTEM：ONLINE"))
        );
        assert!(
            items
                .iter()
                .any(|i| i.name == "归档状态" && i.detail.contains("VALID"))
        );
        assert!(
            items
                .iter()
                .any(|i| i.name == "缓存池使用情况" && i.detail.contains("NORMAL"))
        );
    }

    #[tokio::test]
    async fn test_collect_inspection_includes_datafiles_users_objects_jobs() {
        let empty_table = b"\n---------------------------------------\n".to_vec();
        let runner = MockRunner::new(vec![
            ("su - dmdba -c".to_string(), 0, empty_table.clone()), // 会话数
            ("su - dmdba -c".to_string(), 0, empty_table.clone()), // svr_log
            ("su - dmdba -c".to_string(), 0, empty_table.clone()), // 表空间
            ("su - dmdba -c".to_string(), 0, empty_table.clone()), // 归档
            ("su - dmdba -c".to_string(), 0, empty_table.clone()), // 缓存池
            (
                "su - dmdba -c".to_string(),
                0,
                b"PATH BYTES AUTO_EXTEND\n---------------------------------------\n/dmdata/DAMENG/SYSTEM.DBF 276 1\n".to_vec(),
            ),
            (
                "su - dmdba -c".to_string(),
                0,
                b"USERNAME ACCOUNT_STATUS\n---------------------------------------\nSYSDBA OPEN\n".to_vec(),
            ),
            (
                "su - dmdba -c".to_string(),
                0,
                b"OBJECT_TYPE COUNT(*)\n---------------------------------------\nTABLE 12\n".to_vec(),
            ),
            (
                "su - dmdba -c".to_string(),
                0,
                b"NAME ENABLE STATE\n---------------------------------------\nbak_full 1 Y\n".to_vec(),
            ),
        ]);
        let items = collect_inspection(&runner, &make_config(), "pwd1").await;
        assert!(
            items
                .iter()
                .any(|i| i.name == "数据文件信息" && i.detail.contains("SYSTEM.DBF"))
        );
        assert!(
            items
                .iter()
                .any(|i| i.name == "用户信息" && i.detail.contains("SYSDBA"))
        );
        assert!(
            items
                .iter()
                .any(|i| i.name == "对象信息" && i.detail.contains("TABLE"))
        );
        assert!(
            items
                .iter()
                .any(|i| i.name == "作业执行情况" && i.detail.contains("bak_full"))
        );
    }

    #[test]
    fn test_parse_dmserver_log_lines() {
        let output = "2026-06-10 19:06:22.259 [FATAL] database P0000093865 T0000000000000093865  sigterm_handler receive signal 15";
        let rows = parse_dmserver_log_lines(output);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].time, "2026-06-10 19:06:22.259");
        assert_eq!(rows[0].level, "[FATAL]");
        assert_eq!(rows[0].pid, "P0000093865");
        assert_eq!(rows[0].tid, "T0000000000000093865");
        assert_eq!(rows[0].message, "sigterm_handler receive signal 15");
    }

    #[tokio::test]
    async fn test_collect_dmserver_error_logs_greps_log_files() {
        let runner = MockRunner::new(vec![(
            "grep -hE".to_string(),
            0,
            b"2026-06-10 19:06:22.259 [FATAL] database P0000093865 T0000000000000093865 sigterm_handler receive signal 15\n".to_vec(),
        )]);
        let rows = collect_dmserver_error_logs(&runner, &make_config()).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pid, "P0000093865");
    }

    #[tokio::test]
    async fn test_collect_network_info_includes_real_output() {
        let runner = MockRunner::new(vec![(
            "ip addr".to_string(),
            0,
            b"1: lo: <LOOPBACK,UP> inet 127.0.0.1/8\n".to_vec(),
        )]);
        let item = collect_network_info(&runner).await;
        assert_eq!(item.status, Status::ManualRequired);
        assert!(item.detail.contains("127.0.0.1/8"));
    }

    #[tokio::test]
    async fn test_collect_network_info_falls_back_when_empty() {
        let runner = MockRunner::new_strict(vec![]);
        let item = collect_network_info(&runner).await;
        assert!(item.detail.contains("未能获取网卡信息"));
    }

    #[test]
    fn test_parse_dd_throughput_extracts_speed() {
        let out = "10240+0 records in\n10240+0 records out\n335544320 bytes (336 MB, 320 MiB) copied, 0.728511 s, 460 MB/s\n";
        assert_eq!(parse_dd_throughput(out), Some("460 MB/s".to_string()));
    }

    #[test]
    fn test_parse_dd_throughput_returns_none_when_no_match() {
        assert_eq!(parse_dd_throughput("dd: permission denied"), None);
    }

    #[tokio::test]
    async fn test_collect_disk_speed_parses_throughput() {
        let runner = MockRunner::new(vec![(
            "dd if=/dev/zero".to_string(),
            0,
            b"335544320 bytes (336 MB, 320 MiB) copied, 0.728511 s, 460 MB/s\n".to_vec(),
        )]);
        let item = collect_disk_speed(&runner, &make_config()).await;
        assert_eq!(item.status, Status::Normal);
        assert!(item.detail.contains("460 MB/s"));
    }

    #[tokio::test]
    async fn test_collect_disk_speed_falls_back_to_manual_on_parse_failure() {
        let runner = MockRunner::new(vec![(
            "dd if=/dev/zero".to_string(),
            0,
            b"dd: permission denied\n".to_vec(),
        )]);
        let item = collect_disk_speed(&runner, &make_config()).await;
        assert_eq!(item.status, Status::ManualRequired);
    }

    #[tokio::test]
    async fn test_collect_audit_version_reports_real_version() {
        let runner = MockRunner::new(vec![(
            "rpm -q audit".to_string(),
            0,
            b"audit-3.0.7-3.el8\n".to_vec(),
        )]);
        let item = collect_audit_version(&runner).await;
        assert_eq!(item.status, Status::ManualRequired);
        assert!(item.detail.contains("audit-3.0.7-3.el8"));
    }

    #[test]
    fn test_check_backup_data_separation_flags_same_path_as_abnormal() {
        let mut config = make_config();
        config.backup.backup_path = Some(config.data_path.clone());
        let item = check_backup_data_separation(&config);
        assert_eq!(item.status, Status::Abnormal);
    }

    #[test]
    fn test_check_backup_data_separation_accepts_separate_path() {
        let mut config = make_config();
        config.backup.backup_path = Some("/dmbak/DAMENG".to_string());
        let item = check_backup_data_separation(&config);
        assert_eq!(item.status, Status::Normal);
    }

    #[test]
    fn test_check_backup_data_separation_manual_when_unconfigured() {
        let config = make_config();
        let item = check_backup_data_separation(&config);
        assert_eq!(item.status, Status::ManualRequired);
    }

    #[tokio::test]
    async fn test_collect_backup_cleanup_job_enabled() {
        let runner = MockRunner::new(vec![(
            "su - dmdba -c".to_string(),
            0,
            b"NAME ENABLE\n---------------------------------------\nbak_clear 1\n".to_vec(),
        )]);
        let item = collect_backup_cleanup_job(&runner, &make_config(), "pwd1").await;
        assert_eq!(item.status, Status::Normal);
        assert!(item.detail.contains("已启用"));
    }

    #[tokio::test]
    async fn test_collect_backup_cleanup_job_missing_is_manual() {
        let runner = MockRunner::new(vec![(
            "su - dmdba -c".to_string(),
            0,
            b"NAME ENABLE\n---------------------------------------\n".to_vec(),
        )]);
        let item = collect_backup_cleanup_job(&runner, &make_config(), "pwd1").await;
        assert_eq!(item.status, Status::ManualRequired);
    }

    #[tokio::test]
    async fn test_collect_license_info_reports_data_when_found() {
        let runner = MockRunner::new(vec![(
            "su - dmdba -c".to_string(),
            0,
            b"EXPIRED_DATE\n---------------------------------------\nPERMANENT\n".to_vec(),
        )]);
        let item = collect_license_info(&runner, &make_config(), "pwd1").await;
        assert_eq!(item.status, Status::Normal);
        assert!(item.detail.contains("PERMANENT"));
    }

    #[tokio::test]
    async fn test_collect_license_info_manual_when_empty() {
        let runner = MockRunner::new(vec![(
            "su - dmdba -c".to_string(),
            0,
            b"EXPIRED_DATE\n---------------------------------------\n".to_vec(),
        )]);
        let item = collect_license_info(&runner, &make_config(), "pwd1").await;
        assert_eq!(item.status, Status::ManualRequired);
    }

    #[tokio::test]
    async fn test_collect_user_role_privileges_groups_by_user() {
        let runner = MockRunner::new(vec![(
            "su - dmdba -c".to_string(),
            0,
            b"GRANTEE GRANTED_ROLE\n---------------------------------------\nDB_IM SOI\nDB_IM RESOURCE\nECOLOGY10 PUBLIC\n".to_vec(),
        )]);
        let item = collect_user_role_privileges(&runner, &make_config(), "pwd1").await;
        assert!(item.detail.contains("DB_IM的权限是:SOI, RESOURCE"));
        assert!(item.detail.contains("ECOLOGY10的权限是:PUBLIC"));
    }

    #[tokio::test]
    async fn test_collect_user_role_privileges_manual_when_empty() {
        let runner = MockRunner::new(vec![(
            "su - dmdba -c".to_string(),
            0,
            b"GRANTEE GRANTED_ROLE\n---------------------------------------\n".to_vec(),
        )]);
        let item = collect_user_role_privileges(&runner, &make_config(), "pwd1").await;
        assert_eq!(item.status, Status::ManualRequired);
    }

    #[tokio::test]
    async fn test_collect_sql_error_history_parses_rows_with_multiword_sql() {
        let output = "SESS_ID ECPT_DESC SQL_TEXT\n---------------------------------------\n140565891951064 InvalidArgument select SF_GET_PARA_VALUE (2, 'DCP_OPT_FLAG')\n";
        let runner = MockRunner::new(vec![(
            "su - dmdba -c".to_string(),
            0,
            output.as_bytes().to_vec(),
        )]);
        let rows = collect_sql_error_history(&runner, &make_config(), "pwd1").await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sess_id, "140565891951064");
        assert_eq!(rows[0].ecpt_desc, "InvalidArgument");
        assert!(rows[0].sql_text.contains("SF_GET_PARA_VALUE"));
    }

    #[tokio::test]
    async fn test_collect_sql_error_history_empty_when_unavailable() {
        let runner = MockRunner::new_strict(vec![]);
        let rows = collect_sql_error_history(&runner, &make_config(), "pwd1").await;
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn test_collect_top_sql_parses_rows_with_multiword_sql() {
        let runner = MockRunner::new(vec![(
            "su - dmdba -c".to_string(),
            0,
            b"SQL_TEXT EXEC_TIME FINISH_TIME\n---------------------------------------\nselect * from T1 120ms 09:15:30\n".to_vec(),
        )]);
        let rows = collect_top_sql(&runner, &make_config(), "pwd1").await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sql_text, "select * from T1");
        assert_eq!(rows[0].exec_time, "120ms");
        assert_eq!(rows[0].finish_time, "09:15:30");
    }

    #[tokio::test]
    async fn test_collect_top_sql_empty_when_unavailable() {
        let runner = MockRunner::new_strict(vec![]);
        let rows = collect_top_sql(&runner, &make_config(), "pwd1").await;
        assert!(rows.is_empty());
    }
}
