//! 达梦官方已知问题参数知识库（1.2/1.3 章节"重点参数检查总结"）。
//!
//! 这是固定的官方已知问题清单，不随采集变化；摘自《数据库上线检查报告》模板样例
//! （为可维护性只收录模板中出现的代表性条目，后续可按官方公告增补）。
//! 每条规则在渲染时与已采集的 `V$DM_INI` 现状值（`param_detail`）拼接，
//! 生成最终的 `生产系统中参数值` 列；是否真正命中问题留给人工核对（章节本身即提示
//! "工具检测到以下重点 ini 参数需要您人工核对"）。

use super::model::{KeyParamCheck, ParamCheck};

struct KeyParamRule {
    /// 参数名，多个参数用"、"分隔（对应模板里的组合行，如 "A、B"）。
    names: &'static str,
    bad_value_rule: &'static str,
    recommend: &'static str,
    since_version: &'static str,
    fixed_version: &'static str,
    fixed_release: &'static str,
}

const KEY_PARAMS_2025: &[KeyParamRule] = &[
    KeyParamRule {
        names: "VIEW_PULLUP_FLAG",
        bad_value_rule: "VIEW_PULLUP_FLAG包含2",
        recommend: "VIEW_PULLUP_FLAG=1",
        since_version: "V8.0.0.0",
        fixed_version: "V8.1.4.101",
        fixed_release: "2025年第二季度正式版-8.1.4.116",
    },
    KeyParamRule {
        names: "ENABLE_RQ_TO_INV",
        bad_value_rule: "ENABLE_RQ_TO_INV=[0,1]",
        recommend: "ENABLE_RQ_TO_INV=0",
        since_version: "V8.0.0.0",
        fixed_version: "V8.1.4.89",
        fixed_release: "2025年第一季度正式版-8.1.4.80",
    },
    KeyParamRule {
        names: "BEXP_CALC_ST_FLAG",
        bad_value_rule: "BEXP_CALC_ST_FLAG=128",
        recommend: "BEXP_CALC_ST_FLAG=128",
        since_version: "V8.1.3.35",
        fixed_version: "V8.1.4.87",
        fixed_release: "2025年第一季度正式版-8.1.4.80",
    },
    KeyParamRule {
        names: "STMT_XBOX_REUSE",
        bad_value_rule: "STMT_XBOX_REUSE!=0",
        recommend: "STMT_XBOX_REUSE=1",
        since_version: "V8.1.2.99",
        fixed_version: "V8.1.4.17",
        fixed_release: "2024年第四季度正式版-8.1.4.48",
    },
    KeyParamRule {
        names: "MERGE_OPT_FLAG",
        bad_value_rule: "MERGE_OPT_FLAG=1",
        recommend: "MERGE_OPT_FLAG=1",
        since_version: "V8.1.3.159",
        fixed_version: "V8.1.4.87",
        fixed_release: "2025年第一季度正式版-8.1.4.80",
    },
    KeyParamRule {
        names: "BAK_SAFE_CHECK",
        bad_value_rule: "BAK_SAFE_CHECK!=0",
        recommend: "BAK_SAFE_CHECK=7",
        since_version: "V8.0.0.0",
        fixed_version: "V8.1.4.5",
        fixed_release: "2024年第三季度正式版-8.1.4.6",
    },
    KeyParamRule {
        names: "ENABLE_IN_VALUE_LIST_OPT",
        bad_value_rule: "ENABLE_IN_VALUE_LIST_OPT!=[0,1024]",
        recommend: "ENABLE_IN_VALUE_LIST_OPT=518",
        since_version: "V8.0.0.0",
        fixed_version: "V8.1.3.159",
        fixed_release: "2024年第二季度正式版-8.1.3.162",
    },
    KeyParamRule {
        names: "ENABLE_INDEX_FILTER、OPTIMIZER_OR_NBEXP",
        bad_value_rule: "ENABLE_INDEX_FILETER=2&OPTIMIZER_OR_NBEXP=0",
        recommend: "ENABLE_INDEX_FILTER=1、OPTIMIZER_OR_NBEXP=0",
        since_version: "V8.1.1.151",
        fixed_version: "V8.1.3.45",
        fixed_release: "2023年第三季度正式版-8.1.3.62",
    },
    KeyParamRule {
        names: "ENABLE_MONITOR",
        bad_value_rule: "ENABLE_MONITOR = 1",
        recommend: "ENABLE_MONITOR=0",
        since_version: "V8.1.1.144",
        fixed_version: "V8.1.1.172",
        fixed_release: "V8.1.1.172",
    },
];

const KEY_PARAMS_2024: &[KeyParamRule] = &[
    KeyParamRule {
        names: "HASH_PLL_OPT_FLAG",
        bad_value_rule: "HASH_PLL_OPT_FLAG包含1",
        recommend: "HASH_PLL_OPT_FLAG=107",
        since_version: "V8.1.3.149",
        fixed_version: "V8.1.3.149",
        fixed_release: "2024年第二季度正式版-8.1.3.162",
    },
    KeyParamRule {
        names: "RLOG_RESERVE_THRESHOLD",
        bad_value_rule: "RLOG_RESERVE_THRESHOLD=0",
        recommend: "RLOG_RESERVE_THRESHOLD=0",
        since_version: "8.1.4.5",
        fixed_version: "8.1.4.5",
        fixed_release: "2024年第三季度正式版-8.1.4.6",
    },
    KeyParamRule {
        names: "RLOG_CHECK_SPACE",
        bad_value_rule: "RLOG_CHECK_SPACE!=2",
        recommend: "RLOG_CHECK_SPACE=1",
        since_version: "V8.1.4.5",
        fixed_version: "V8.1.4.5",
        fixed_release: "2024年第三季度正式版-8.1.4.6",
    },
    KeyParamRule {
        names: "PTX_ROLLBACK",
        bad_value_rule: "PTX_ROLLBACK!=1",
        recommend: "PTX_ROLLBACK=0",
        since_version: "8.1.4.5",
        fixed_version: "8.1.4.5",
        fixed_release: "2024年第三季度正式版-8.1.4.6",
    },
    KeyParamRule {
        names: "NBEXP_OPT_FLAG、SPEED_SEMI_JOIN_PLAN",
        bad_value_rule: "NBEXP_OPT_FLAG不包含16&SPEED_SEMI_JOIN_PLAN不包含32",
        recommend: "NBEXP_OPT_FLAG=7、SPEED_SEMI_JOIN_PLAN=9",
        since_version: "V8.1.3.193",
        fixed_version: "V8.1.3.193",
        fixed_release: "2024年第三季度正式版-8.1.4.6",
    },
    KeyParamRule {
        names: "VIEW_PULLUP_FLAG",
        bad_value_rule: "VIEW_PULLUP_FLAG 不包含2",
        recommend: "VIEW_PULLUP_FLAG=1",
        since_version: "V8.1.3.163",
        fixed_version: "V8.1.3.163",
        fixed_release: "2024年第三季度正式版-8.1.4.6",
    },
    KeyParamRule {
        names: "PARTIAL_JOIN_EVALUATION_FLAG",
        bad_value_rule: "PARTIAL_JOIN_EVALUATION_FLAG!=0",
        recommend: "PARTIAL_JOIN_EVALUATION_FLAG=1",
        since_version: "V8.1.3.155",
        fixed_version: "V8.1.3.155",
        fixed_release: "2024年第二季度正式版-8.1.3.162",
    },
    KeyParamRule {
        names: "SORT_FLAG",
        bad_value_rule: "SORT_FLAG=0",
        recommend: "SORT_FLAG=0",
        since_version: "8.1.4.21",
        fixed_version: "8.1.4.21",
        fixed_release: "2024年第四季度正式版-8.1.4.48",
    },
    KeyParamRule {
        names: "ENABLE_JOIN_FACTORIZATION",
        bad_value_rule: "ENABLE_JOIN_FACTORIZATION!=0",
        recommend: "ENABLE_JOIN_FACTORIZATION=1",
        since_version: "V8.1.4.43",
        fixed_version: "V8.1.4.43",
        fixed_release: "2024年第四季度正式版-8.1.4.48",
    },
];

/// 按规则名查表，把已采集的 `param_detail` 现状值拼成"生产系统中参数值"列。
fn lookup_prod_value(names: &str, param_detail: &[ParamCheck]) -> String {
    let found: Vec<String> = names
        .split('、')
        .filter_map(|name| {
            param_detail
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(name.trim()))
                .map(|p| format!("{}={}", p.name, p.para_value))
        })
        .collect();
    if found.is_empty() {
        "未采集".to_string()
    } else {
        found.join("、")
    }
}

fn build(rules: &[KeyParamRule], param_detail: &[ParamCheck]) -> Vec<KeyParamCheck> {
    rules
        .iter()
        .map(|r| KeyParamCheck {
            name: r.names.to_string(),
            bad_value_rule: r.bad_value_rule.to_string(),
            recommend: r.recommend.to_string(),
            prod_value: lookup_prod_value(r.names, param_detail),
            since_version: r.since_version.to_string(),
            fixed_version: r.fixed_version.to_string(),
            fixed_release: r.fixed_release.to_string(),
        })
        .collect()
}

/// 1.2、2025重点参数检查总结。
pub fn key_params_2025(param_detail: &[ParamCheck]) -> Vec<KeyParamCheck> {
    build(KEY_PARAMS_2025, param_detail)
}

/// 1.3、2024重点参数检查总结。
pub fn key_params_2024(param_detail: &[ParamCheck]) -> Vec<KeyParamCheck> {
    build(KEY_PARAMS_2024, param_detail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_params_2025_includes_enable_monitor() {
        let checks = key_params_2025(&[]);
        assert!(checks.iter().any(|c| c.name == "ENABLE_MONITOR"));
    }

    #[test]
    fn test_lookup_prod_value_joins_combo_params() {
        let detail = vec![
            ParamCheck {
                name: "NBEXP_OPT_FLAG".into(),
                para_value: "7".into(),
                file_value: "7".into(),
                recommend: "7".into(),
            },
            ParamCheck {
                name: "SPEED_SEMI_JOIN_PLAN".into(),
                para_value: "9".into(),
                file_value: "9".into(),
                recommend: "9".into(),
            },
        ];
        let value = lookup_prod_value("NBEXP_OPT_FLAG、SPEED_SEMI_JOIN_PLAN", &detail);
        assert_eq!(value, "NBEXP_OPT_FLAG=7、SPEED_SEMI_JOIN_PLAN=9");
    }

    #[test]
    fn test_lookup_prod_value_missing_returns_placeholder() {
        assert_eq!(lookup_prod_value("UNKNOWN_PARAM", &[]), "未采集");
    }
}
