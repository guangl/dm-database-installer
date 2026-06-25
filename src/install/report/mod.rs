pub mod collect;
pub mod knowledge;
pub mod model;
pub mod render;

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::config::{InstallConfig, ReportConfig};
use crate::ssh::CommandRunner;
use model::{CheckItem, ReportData, ReportMeta, SqlErrorRow, Status, TopSqlRow};

/// 报告输出文件名：数据库上线检查报告_{host}_{YYYYMMDD}.docx。
pub fn report_filename(host: &str, date_yyyymmdd: &str) -> PathBuf {
    PathBuf::from(format!("数据库上线检查报告_{host}_{date_yyyymmdd}.docx"))
}

/// 渲染报告。`data.summary` 会在渲染前被重新派生，调用方无需手动填充。
pub fn generate(mut data: ReportData, output_path: &Path) -> Result<PathBuf> {
    data.summary = model::derive_summary(&data);
    render::render(&data, output_path)
}

/// `collect::collect_sql_error_history` 查询的 `V$SQL_ERROR_HISTORY` 视图名未经过
/// 真实达梦实例验证；查询失败/返回空时用这条占位说明，而不是留一张空表。
fn manual_sql_error_placeholder() -> Vec<SqlErrorRow> {
    vec![SqlErrorRow {
        sess_id: "-".to_string(),
        ecpt_desc: "未采集到数据".to_string(),
        sql_text: "未查询到历史 SQL 错误（或目标系统视图不存在），请人工核对。".to_string(),
    }]
}

/// `collect::collect_top_sql` 查询的 `V$LONG_EXEC_SQLS` 视图名未经过真实达梦实例
/// 验证；查询失败/返回空时用这条占位说明。
fn manual_top_sql_placeholder() -> Vec<TopSqlRow> {
    vec![TopSqlRow {
        sql_text: "未查询到 top sql 数据（或目标系统视图不存在/未开启 SQL 监控），请人工核对。"
            .to_string(),
        exec_time: "-".to_string(),
        finish_time: "-".to_string(),
    }]
}

/// 没有数据来源支撑、确实只能靠人工的两项：异地备份需要外部远程目标信息（config
/// 里没有这类配置），应用驱动版本需要应用侧信息，本工具拿不到。
fn manual_db_check_items() -> Vec<CheckItem> {
    vec![
        CheckItem::new(
            "归档和备份是否定期异地备份",
            Status::ManualRequired,
            "请人工确认归档和备份是否有定期拷贝到远程服务器进行异地备份",
        ),
        CheckItem::new(
            "应用数据库驱动版本与数据库版本",
            Status::ManualRequired,
            "请人工确认应用数据库驱动版本与数据库版本是否统一",
        ),
    ]
}

/// 安装完成后采集数据并生成报告。失败不应阻断安装流程，调用方按需把 Err 降级为 warn 日志。
pub async fn generate_for_install(
    runner: &dyn CommandRunner,
    install_config: &InstallConfig,
    report_cfg: &ReportConfig,
    sysdba_pwd: &str,
    dm_version: Option<&str>,
    today_yyyymmdd: &str,
) -> Result<PathBuf> {
    let host = "127.0.0.1".to_string();

    let mut os_checks = collect::collect_os_checks(runner).await;
    os_checks.push(collect::collect_network_info(runner).await);
    os_checks.push(collect::collect_disk_speed(runner, install_config).await);
    os_checks.push(collect::collect_audit_version(runner).await);

    let mut db_checks = manual_db_check_items();
    db_checks.push(collect::check_backup_data_separation(install_config));
    db_checks.push(collect::collect_backup_cleanup_job(runner, install_config, sysdba_pwd).await);
    db_checks.push(collect::collect_license_info(runner, install_config, sysdba_pwd).await);
    db_checks.push(collect::collect_user_role_privileges(runner, install_config, sysdba_pwd).await);
    if let Some(v) = dm_version {
        db_checks.push(CheckItem::new("数据库版本", Status::Normal, v.to_string()));
    }

    let param_detail = collect::collect_param_detail(runner, install_config, sysdba_pwd).await;
    let key_params_2025 = knowledge::key_params_2025(&param_detail);
    let key_params_2024 = knowledge::key_params_2024(&param_detail);

    let mut instance_info = vec![
        ("数据库名".to_string(), "DAMENG".to_string()),
        ("端口号".to_string(), install_config.port.to_string()),
        ("实例名".to_string(), install_config.instance_name.clone()),
        (
            "版本号".to_string(),
            dm_version.unwrap_or("未采集").to_string(),
        ),
    ];
    instance_info
        .extend(collect::collect_instance_status(runner, install_config, sysdba_pwd).await);

    let inspection = collect::collect_inspection(runner, install_config, sysdba_pwd).await;
    let dmserver_logs = collect::collect_dmserver_error_logs(runner, install_config).await;
    let dmap_logs = collect::collect_dmap_error_logs(runner, install_config).await;

    let sql_errors = {
        let rows = collect::collect_sql_error_history(runner, install_config, sysdba_pwd).await;
        if rows.is_empty() {
            manual_sql_error_placeholder()
        } else {
            rows
        }
    };
    let top_sql = {
        let rows = collect::collect_top_sql(runner, install_config, sysdba_pwd).await;
        if rows.is_empty() {
            manual_top_sql_placeholder()
        } else {
            rows
        }
    };

    let data = ReportData {
        meta: ReportMeta {
            system_name: report_cfg.system_name.clone(),
            user_org: report_cfg.user_org.clone(),
            inspect_org: report_cfg.inspect_org.clone(),
            engineer: report_cfg.engineer.clone(),
            inspect_time: today_yyyymmdd.to_string(),
            host: host.clone(),
        },
        summary: model::SummaryData::default(),
        os_checks,
        db_checks,
        key_params_2025,
        key_params_2024,
        install_detail: Vec::new(),
        instance_info,
        inspection,
        param_detail,
        sql_errors,
        top_sql,
        dmap_logs,
        dmserver_logs,
    };

    let output_dir = report_cfg
        .output_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&output_dir)?;
    let output_path = output_dir.join(report_filename(&host, today_yyyymmdd));

    generate(data, &output_path)
}

#[cfg(test)]
mod tests {
    use super::model::*;
    use super::*;

    fn sample_data() -> ReportData {
        ReportData {
            meta: ReportMeta {
                system_name: "测试系统".into(),
                user_org: "售后服务部".into(),
                inspect_org: "武汉达梦数据库股份有限公司".into(),
                engineer: "".into(),
                inspect_time: "2026-06-25".into(),
                host: "127.0.0.1".into(),
            },
            summary: SummaryData::default(),
            os_checks: vec![
                CheckItem::new("glibc 版本", Status::Abnormal, "ldd (GNU libc) 2.28"),
                CheckItem::new("SELinux", Status::Normal, "Disabled"),
            ],
            db_checks: vec![CheckItem::new(
                "应用用户权限配置",
                Status::ManualRequired,
                "请人工核对应用用户是否存在超出权限配置",
            )],
            key_params_2025: super::knowledge::key_params_2025(&[ParamCheck {
                name: "ENABLE_MONITOR".into(),
                para_value: "1".into(),
                file_value: "1".into(),
                recommend: "0".into(),
            }]),
            key_params_2024: super::knowledge::key_params_2024(&[]),
            install_detail: vec![CheckItem::new(
                "防火墙现在的状态",
                Status::Normal,
                "Active: inactive (dead)",
            )],
            instance_info: vec![
                ("数据库名".into(), "DAMENG".into()),
                ("系统状态".into(), "OPEN".into()),
                ("实例模式".into(), "NORMAL".into()),
            ],
            inspection: vec![
                CheckItem::new(
                    "dmdba用户的密码非默认密码",
                    Status::Normal,
                    "用户的密码禁止为默认的密码",
                ),
                CheckItem::new("会话数使用", Status::Normal, "目前会话数 2"),
                CheckItem::new(
                    "SQL日志策略（SVR_LOG）",
                    Status::Warning,
                    "SVR_LOG=1（生产环境建议关闭，设为 0）",
                ),
            ],
            param_detail: vec![ParamCheck {
                name: "MEMORY_TARGET".into(),
                para_value: "5000".into(),
                file_value: "5000".into(),
                recommend: "4000".into(),
            }],
            sql_errors: vec![],
            top_sql: vec![],
            dmap_logs: vec![],
            dmserver_logs: vec![LogErrorRow {
                time: "2026-06-10 19:06:22.259".into(),
                level: "[FATAL]".into(),
                pid: "P0000093865".into(),
                tid: "T0000000000000093865".into(),
                message: "sigterm_handler receive signal 15".into(),
            }],
        }
    }

    #[test]
    fn test_generate_produces_valid_docx() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.docx");
        let result = generate(sample_data(), &path).unwrap();
        assert_eq!(result, path);
        assert!(path.exists());

        let file = std::fs::File::open(&path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let mut doc = zip.by_name("word/document.xml").unwrap();
        let mut content = String::new();
        std::io::Read::read_to_string(&mut doc, &mut content).unwrap();

        assert!(content.contains("数据库上线检查报告"));
        assert!(content.contains("MEMORY_TARGET"));
        assert!(content.contains("sigterm_handler"));
        assert!(content.contains("ENABLE_MONITOR"));
    }

    #[test]
    fn test_derive_summary_collects_review_items() {
        let summary = derive_summary(&sample_data());
        let names: Vec<_> = summary
            .review_items
            .iter()
            .map(|i| i.name.clone())
            .collect();
        assert!(names.contains(&"glibc 版本".to_string()));
        assert!(names.contains(&"MEMORY_TARGET".to_string()));
        assert!(names.contains(&"dm_DMSERVER 运行错误日志".to_string()));
    }

    #[test]
    fn test_report_filename_format() {
        let path = report_filename("127.0.0.1", "20260625");
        assert_eq!(
            path.to_str().unwrap(),
            "数据库上线检查报告_127.0.0.1_20260625.docx"
        );
    }
}
