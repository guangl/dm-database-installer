//! 用 docx-rs 把 ReportData 渲染成 .docx，章节顺序与原模板一致。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use docx_rs::{AlignmentType, Docx, Paragraph, Run, Table, TableCell, TableRow, VAlignType};

use super::model::{
    CheckItem, KeyParamCheck, LogErrorRow, ParamCheck, ReportData, SqlErrorRow, Status, TopSqlRow,
};

const COLOR_NORMAL: &str = "000000";
const COLOR_WARNING: &str = "B45309";
const COLOR_ABNORMAL: &str = "C00000";
const COLOR_MANUAL: &str = "808080";

fn status_color(status: Status) -> &'static str {
    match status {
        Status::Normal => COLOR_NORMAL,
        Status::Warning => COLOR_WARNING,
        Status::Abnormal => COLOR_ABNORMAL,
        Status::ManualRequired => COLOR_MANUAL,
    }
}

fn heading(text: &str, size: usize) -> Paragraph {
    Paragraph::new()
        .add_run(Run::new().add_text(text).bold().size(size))
        .align(AlignmentType::Left)
}

fn para(text: &str) -> Paragraph {
    Paragraph::new().add_run(Run::new().add_text(text))
}

fn cell(text: &str) -> TableCell {
    TableCell::new()
        .vertical_align(VAlignType::Center)
        .add_paragraph(para(text))
}

fn status_cell(status: Status) -> TableCell {
    TableCell::new()
        .vertical_align(VAlignType::Center)
        .add_paragraph(
            Paragraph::new().add_run(
                Run::new()
                    .add_text(status.label())
                    .color(status_color(status)),
            ),
        )
}

fn header_row(labels: &[&str]) -> TableRow {
    TableRow::new(
        labels
            .iter()
            .map(|l| {
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(Run::new().add_text(*l).bold()))
            })
            .collect(),
    )
}

fn check_item_table(items: &[CheckItem]) -> Table {
    let mut rows = vec![header_row(&["检查内容", "状态", "结果说明"])];
    for item in items {
        rows.push(TableRow::new(vec![
            cell(&item.name),
            status_cell(item.status),
            cell(&item.detail),
        ]));
    }
    Table::new(rows)
}

fn param_table(items: &[ParamCheck]) -> Table {
    let mut rows = vec![header_row(&["参数名称", "参数值", "推荐值"])];
    for p in items {
        rows.push(TableRow::new(vec![
            cell(&p.name),
            cell(&format!(
                "PARA_VALUE={} FILE_VALUE={}",
                p.para_value, p.file_value
            )),
            cell(&p.recommend),
        ]));
    }
    Table::new(rows)
}

fn key_param_table(items: &[KeyParamCheck]) -> Table {
    let mut rows = vec![header_row(&[
        "参数名称",
        "参数问题值",
        "参数推荐值",
        "生产系统中参数值",
        "引入版本",
        "解决版本",
        "解决的正式版本",
    ])];
    for p in items {
        rows.push(TableRow::new(vec![
            cell(&p.name),
            cell(&p.bad_value_rule),
            cell(&p.recommend),
            cell(&p.prod_value),
            cell(&p.since_version),
            cell(&p.fixed_version),
            cell(&p.fixed_release),
        ]));
    }
    Table::new(rows)
}

fn instance_info_table(items: &[(String, String)]) -> Table {
    let mut rows = vec![header_row(&["参数名称", "结果说明"])];
    for (name, value) in items {
        rows.push(TableRow::new(vec![cell(name), cell(value)]));
    }
    Table::new(rows)
}

fn sql_error_table(items: &[SqlErrorRow]) -> Table {
    let mut rows = vec![header_row(&["SESS_ID", "ECPT_DESC", "SQL_TEXT"])];
    for r in items {
        rows.push(TableRow::new(vec![
            cell(&r.sess_id),
            cell(&r.ecpt_desc),
            cell(&r.sql_text),
        ]));
    }
    Table::new(rows)
}

fn top_sql_table(items: &[TopSqlRow]) -> Table {
    let mut rows = vec![header_row(&["SQL_TEXT", "EXEC_TIME", "FINISH_TIME"])];
    for r in items {
        rows.push(TableRow::new(vec![
            cell(&r.sql_text),
            cell(&r.exec_time),
            cell(&r.finish_time),
        ]));
    }
    Table::new(rows)
}

fn log_error_table(items: &[LogErrorRow]) -> Table {
    let mut rows = vec![header_row(&[
        "时间",
        "级别",
        "进程号",
        "线程号",
        "错误信息",
    ])];
    for r in items {
        rows.push(TableRow::new(vec![
            cell(&r.time),
            cell(&r.level),
            cell(&r.pid),
            cell(&r.tid),
            cell(&r.message),
        ]));
    }
    Table::new(rows)
}

pub fn render(data: &ReportData, output_path: &Path) -> Result<PathBuf> {
    let mut docx = Docx::new();

    docx = docx
        .add_paragraph(heading("数据库上线检查报告", 36))
        .add_paragraph(para(&format!("系统名称：{}", data.meta.system_name)))
        .add_paragraph(para(&format!("用户单位：{}", data.meta.user_org)))
        .add_paragraph(para(&format!("巡检单位：{}", data.meta.inspect_org)))
        .add_paragraph(para(&format!("巡检工程师：{}", data.meta.engineer)))
        .add_paragraph(para(&format!("巡检时间：{}", data.meta.inspect_time)));

    docx = docx
        .add_paragraph(heading("一、 检查总结", 28))
        .add_paragraph(para(
            "本章节内容为此次上线检查过程中不符合达梦数据库运行的项和需要人工确认的项的总结，如无特殊情况，可重点评审本章节内容，最好对本章节内容进行人工复检！",
        ))
        .add_paragraph(para("确认人："))
        .add_paragraph(para("上线时间："))
        .add_paragraph(para("评审结论："))
        .add_paragraph(para("是否建议召开评审会议：建议"))
        .add_paragraph(para(
            "【待开会评审项】以下检查项存在异常或与推荐值不一致，请组织上线评审会议对相应检查项进行评审！",
        ))
        .add_table(check_item_table(&data.summary.review_items));

    docx = docx
        .add_paragraph(heading(&format!("1.1、{}", data.meta.host), 24))
        .add_paragraph(heading("1.1.1、数据安装检查总结", 22))
        .add_table(check_item_table(&data.os_checks))
        .add_paragraph(heading("1.1.2、数据库检查总结", 22))
        .add_table(check_item_table(&data.db_checks));

    docx = docx
        .add_paragraph(heading("1.2、2025重点参数检查总结", 22))
        .add_paragraph(para("工具检测到以下重点 ini 参数需要您人工核对。"))
        .add_table(key_param_table(&data.key_params_2025))
        .add_paragraph(heading("1.3、2024重点参数检查总结", 22))
        .add_paragraph(para("工具检测到以下重点 ini 参数需要您人工核对。"))
        .add_table(key_param_table(&data.key_params_2024));

    docx = docx
        .add_paragraph(heading("二、 检查详细内容", 28))
        .add_paragraph(heading(&format!("2.2、{}", data.meta.host), 24))
        .add_paragraph(heading("2.2.1、数据库安装信息检查详细内容", 22))
        .add_table(check_item_table(&data.install_detail))
        .add_paragraph(heading("2.2.2、数据库实例内容", 22))
        .add_table(instance_info_table(&data.instance_info))
        .add_paragraph(heading("2.2.3、数据库巡检内容", 22))
        .add_table(check_item_table(&data.inspection))
        .add_paragraph(heading("2.2.4、参数配置信息", 22))
        .add_table(param_table(&data.param_detail))
        .add_paragraph(heading("2.2.5、历史SQL错误", 22))
        .add_table(sql_error_table(&data.sql_errors))
        .add_paragraph(heading("2.2.6、top sql信息", 22))
        .add_table(top_sql_table(&data.top_sql))
        .add_paragraph(heading("2.2.7、错误日志信息", 22))
        .add_paragraph(heading("2.2.7.1、dm_dmap运行错误日志", 20))
        .add_table(log_error_table(&data.dmap_logs))
        .add_paragraph(heading("2.2.7.2、dm_DMSERVER运行错误日志", 20))
        .add_table(log_error_table(&data.dmserver_logs));

    docx = docx
        .add_paragraph(heading("三、 附录", 28))
        .add_paragraph(para("其他未尽情况说明和截图。"));

    let file = std::fs::File::create(output_path)
        .with_context(|| format!("无法创建报告文件: {}", output_path.display()))?;
    docx.build()
        .pack(file)
        .map_err(|e| anyhow::anyhow!("生成 docx 失败: {e:?}"))?;
    Ok(output_path.to_path_buf())
}
