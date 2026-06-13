use anyhow::{Context, Result};
use std::process::Command;

use crate::config::InstallConfig;

/// 执行 dminit 初始化达梦数据库实例。
///
/// 关键约束（Pitfall 2）：dminit 参数等号两侧不能有空格。
/// 每个参数用 `.arg(format!("KEY={}", value))` 单独传递。
pub fn run_dminit(config: &InstallConfig, sysdba_pwd: &str, sysauditor_pwd: &str) -> Result<()> {
    let parts = build_dminit_command(config, sysdba_pwd, sysauditor_pwd);
    let dminit_bin = &parts[0];

    let status = Command::new(dminit_bin)
        .args(&parts[1..])
        .status()
        .with_context(|| format!("执行 dminit 失败: {}", dminit_bin))?;

    anyhow::ensure!(
        status.success(),
        "dminit 返回非零退出码: {:?}",
        status.code()
    );
    Ok(())
}

/// 构建 dminit 命令参数列表（测试用）。
///
/// 返回 Vec<String>：[0] = dminit 二进制路径，[1..] = KEY=value 参数（无空格）。
pub(crate) fn build_dminit_command(
    config: &InstallConfig,
    sysdba_pwd: &str,
    sysauditor_pwd: &str,
) -> Vec<String> {
    let dminit_bin = format!("{}/bin/dminit", config.install_path);
    vec![
        dminit_bin,
        format!("PATH={}", config.data_path),
        "DB_NAME=DAMENG".to_string(),
        format!("INSTANCE_NAME={}", config.instance_name),
        format!("PORT_NUM={}", config.port),
        format!("PAGE_SIZE={}", config.page_size),
        format!("EXTENT_SIZE={}", config.extent_size),
        format!("CHARSET={}", config.charset),
        format!("CASE_SENSITIVE={}", if config.case_sensitive { "Y" } else { "N" }),
        format!("SYSDBA_PWD={}", sysdba_pwd),
        format!("SYSAUDITOR_PWD={}", sysauditor_pwd),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(config: &InstallConfig) -> Vec<String> {
        build_dminit_command(config, "DMAdmin1@2024", "AuditAdmin2#2024")
    }

    #[test]
    fn test_build_dminit_command_no_spaces_in_kv() {
        let args = cmd(&InstallConfig::default());
        for arg in &args[1..] {
            assert!(!arg.contains(" = "), "KV 参数不能含 ' = ': {}", arg);
            assert!(!arg.contains("= "), "KV 参数不能含 '= ': {}", arg);
            assert!(!arg.contains(" ="), "KV 参数不能含 ' =': {}", arg);
        }
    }

    #[test]
    fn test_build_dminit_command_includes_all_required_keys() {
        let all_args = cmd(&InstallConfig::default())[1..].join(" ");
        assert!(all_args.contains("PATH="), "缺少 PATH 参数");
        assert!(all_args.contains("DB_NAME="), "缺少 DB_NAME 参数");
        assert!(all_args.contains("INSTANCE_NAME="), "缺少 INSTANCE_NAME 参数");
        assert!(all_args.contains("PORT_NUM="), "缺少 PORT_NUM 参数");
        assert!(all_args.contains("PAGE_SIZE="), "缺少 PAGE_SIZE 参数");
        assert!(all_args.contains("EXTENT_SIZE="), "缺少 EXTENT_SIZE 参数");
        assert!(all_args.contains("CHARSET="), "缺少 CHARSET 参数");
        assert!(all_args.contains("CASE_SENSITIVE="), "缺少 CASE_SENSITIVE 参数");
        assert!(all_args.contains("SYSDBA_PWD=DMAdmin1@2024"), "缺少或错误的 SYSDBA_PWD");
        assert!(all_args.contains("SYSAUDITOR_PWD=AuditAdmin2#2024"), "缺少或错误的 SYSAUDITOR_PWD");
    }

    #[test]
    fn test_build_dminit_command_first_is_binary_path() {
        let args = cmd(&InstallConfig::default());
        assert!(!args.is_empty(), "命令参数列表不能为空");
        assert!(args[0].ends_with("/bin/dminit"), "第一个元素应以 /bin/dminit 结尾，实际: {}", args[0]);
    }
}
