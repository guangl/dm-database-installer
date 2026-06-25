//! 上线检查报告的数据结构。对应模板各章节字段，字段名与模板列名保持一致。

/// 检查项状态。颜色映射见 render.rs。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Normal,
    Warning,
    Abnormal,
    /// 无法自动采集（网络测速、磁盘测速、安全漏洞判断等），需人工核对。
    ManualRequired,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Normal => "正常",
            Status::Warning => "警告",
            Status::Abnormal => "异常",
            Status::ManualRequired => "请人工核对",
        }
    }

    pub fn needs_review(self) -> bool {
        matches!(self, Status::Warning | Status::Abnormal)
    }
}

/// 三列检查表的一行：检查内容 / 状态 / 结果说明。
#[derive(Debug, Clone)]
pub struct CheckItem {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

impl CheckItem {
    pub fn new(name: impl Into<String>, status: Status, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
        }
    }
}

/// dm.ini 参数对比行（2.2.4 参数配置信息）。
#[derive(Debug, Clone)]
pub struct ParamCheck {
    pub name: String,
    pub para_value: String,
    pub file_value: String,
    pub recommend: String,
}

impl ParamCheck {
    pub fn mismatched(&self) -> bool {
        self.para_value != self.recommend
    }
}

/// 重点参数核查行（1.2/1.3 章节）。
#[derive(Debug, Clone)]
pub struct KeyParamCheck {
    pub name: String,
    pub bad_value_rule: String,
    pub recommend: String,
    pub prod_value: String,
    pub since_version: String,
    pub fixed_version: String,
    pub fixed_release: String,
}

#[derive(Debug, Clone)]
pub struct SqlErrorRow {
    pub sess_id: String,
    pub ecpt_desc: String,
    pub sql_text: String,
}

#[derive(Debug, Clone)]
pub struct TopSqlRow {
    pub sql_text: String,
    pub exec_time: String,
    pub finish_time: String,
}

#[derive(Debug, Clone)]
pub struct LogErrorRow {
    pub time: String,
    pub level: String,
    pub pid: String,
    pub tid: String,
    pub message: String,
}

/// 报告抬头信息。
#[derive(Debug, Clone)]
pub struct ReportMeta {
    pub system_name: String,
    pub user_org: String,
    pub inspect_org: String,
    pub engineer: String,
    pub inspect_time: String,
    pub host: String,
}

/// 第一章"检查总结"——异常/待评审项的汇总，由其余章节派生，不是独立数据源。
#[derive(Debug, Clone, Default)]
pub struct SummaryData {
    pub review_items: Vec<CheckItem>,
}

#[derive(Debug, Clone)]
pub struct ReportData {
    pub meta: ReportMeta,
    pub summary: SummaryData,
    /// 1.1.1 数据安装检查总结
    pub os_checks: Vec<CheckItem>,
    /// 1.1.2 数据库检查总结
    pub db_checks: Vec<CheckItem>,
    /// 1.2 重点参数检查总结（2025）
    pub key_params_2025: Vec<KeyParamCheck>,
    /// 1.3 重点参数检查总结（2024）
    pub key_params_2024: Vec<KeyParamCheck>,
    /// 2.2.1 数据库安装信息检查详细内容
    pub install_detail: Vec<CheckItem>,
    /// 2.2.2 数据库实例内容（参数名称/结果说明）
    pub instance_info: Vec<(String, String)>,
    /// 2.2.3 数据库巡检内容
    pub inspection: Vec<CheckItem>,
    /// 2.2.4 参数配置信息（全量 ini 参数）
    pub param_detail: Vec<ParamCheck>,
    /// 2.2.5 历史 SQL 错误
    pub sql_errors: Vec<SqlErrorRow>,
    /// 2.2.6 top sql 信息
    pub top_sql: Vec<TopSqlRow>,
    /// 2.2.7.1 dm_dmap 运行错误日志
    pub dmap_logs: Vec<LogErrorRow>,
    /// 2.2.7.2 dm_DMSERVER 运行错误日志
    pub dmserver_logs: Vec<LogErrorRow>,
}

/// 从各章节里筛出异常/待人工确认项，生成第一章"检查总结"表。
/// 不是独立的数据源，而是对已采集字段的派生汇总——和官方模板的做法一致。
pub fn derive_summary(data: &ReportData) -> SummaryData {
    let mut review_items = Vec::new();
    review_items.extend(
        data.os_checks
            .iter()
            .chain(data.db_checks.iter())
            .chain(data.install_detail.iter())
            .chain(data.inspection.iter())
            .filter(|item| item.status.needs_review())
            .cloned(),
    );
    review_items.extend(
        data.param_detail
            .iter()
            .filter(|p| p.mismatched())
            .map(|p| {
                CheckItem::new(
                    p.name.clone(),
                    Status::Warning,
                    format!("PARA_VALUE={} FILE_VALUE={}", p.para_value, p.file_value),
                )
            }),
    );
    if !data.dmserver_logs.is_empty() {
        review_items.push(CheckItem::new(
            "dm_DMSERVER 运行错误日志",
            Status::Abnormal,
            data.dmserver_logs
                .iter()
                .map(|l| format!("{} [{}] {}", l.time, l.level, l.message))
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    SummaryData { review_items }
}
