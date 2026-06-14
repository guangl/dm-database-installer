use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::config::InstallConfig;

/// 执行 dminit 初始化达梦数据库实例。
///
/// 关键约束（Pitfall 2）：dminit 参数等号两侧不能有空格。
/// 每个参数用 `.arg(format!("KEY={}", value))` 单独传递。
pub fn run_dminit(config: &InstallConfig, sysdba_pwd: &str, sysauditor_pwd: &str) -> Result<()> {
    let parts = build_dminit_command(config, sysdba_pwd, sysauditor_pwd);
    let dminit_bin = &parts[0];
    tracing::debug!(
        "执行 dminit: {} INSTANCE_NAME={} PORT_NUM={} PATH={}",
        dminit_bin,
        config.instance_name,
        config.port,
        config.data_path
    );

    let status = Command::new(dminit_bin)
        .args(&parts[1..])
        .status()
        .with_context(|| format!("执行 dminit 失败: {}", dminit_bin))?;

    tracing::debug!("dminit 退出码: {:?}", status.code());
    anyhow::ensure!(
        status.success(),
        "dminit 返回非零退出码: {:?}",
        status.code()
    );
    tracing::info!("dminit 初始化成功: 实例 {} 端口 {}", config.instance_name, config.port);
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
        "ARCH_INI=1".to_string(),
        format!("SYSDBA_PWD={}", sysdba_pwd),
        format!("SYSAUDITOR_PWD={}", sysauditor_pwd),
    ]
}

/// 生成单机 dmarch.ini 内容（仅本地归档，无 REALTIME 段）。
pub fn generate_standalone_dmarch_ini(config: &InstallConfig) -> String {
    let arch_path = crate::config::resolve_arch_path(&config.archive, &config.data_path);
    crate::config::format_local_arch_section(&arch_path, &config.archive)
}

/// 创建归档目录并写入 dmarch.ini 到 data_path。
pub fn write_dmarch_ini(config: &InstallConfig) -> Result<()> {
    let arch_path = crate::config::resolve_arch_path(&config.archive, &config.data_path);
    std::fs::create_dir_all(&arch_path)
        .with_context(|| format!("创建归档目录失败: {}", arch_path))?;
    let content = generate_standalone_dmarch_ini(config);
    let dmarch_path = Path::new(&config.data_path).join("dmarch.ini");
    std::fs::write(&dmarch_path, &content)
        .with_context(|| format!("写入 dmarch.ini 失败: {}", dmarch_path.display()))?;
    tracing::info!("dmarch.ini 写入完成: {}", dmarch_path.display());
    Ok(())
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
        assert!(all_args.contains("ARCH_INI=1"), "缺少 ARCH_INI=1 参数");
        assert!(all_args.contains("SYSDBA_PWD=DMAdmin1@2024"), "缺少或错误的 SYSDBA_PWD");
        assert!(all_args.contains("SYSAUDITOR_PWD=AuditAdmin2#2024"), "缺少或错误的 SYSAUDITOR_PWD");
    }

    #[test]
    fn test_build_dminit_command_first_is_binary_path() {
        let args = cmd(&InstallConfig::default());
        assert!(!args.is_empty(), "命令参数列表不能为空");
        assert!(args[0].ends_with("/bin/dminit"), "第一个元素应以 /bin/dminit 结尾，实际: {}", args[0]);
    }

    #[test]
    fn test_generate_standalone_dmarch_ini_default_arch_path() {
        let config = InstallConfig::default();
        let ini = generate_standalone_dmarch_ini(&config);
        assert!(ini.contains("ARCH_TYPE = LOCAL"), "应含 LOCAL 归档段");
        assert!(
            ini.contains(&format!("{}/arch", config.data_path)),
            "默认归档路径应为 data_path/arch"
        );
        assert!(ini.contains("ARCH_FILE_SIZE = 128"), "默认文件大小应为 128");
        assert!(ini.contains("ARCH_HANG_FLAG = 0"), "单机默认不挂起");
    }

    #[test]
    fn test_generate_standalone_dmarch_ini_custom_arch_path() {
        use crate::config::ArchiveConfig;
        let config = InstallConfig {
            archive: ArchiveConfig {
                arch_path: Some("/data/myarch".to_string()),
                file_size: 256,
                space_limit: 2048,
                hang_flag: true,
                compressed: true,
            },
            ..Default::default()
        };
        let ini = generate_standalone_dmarch_ini(&config);
        assert!(ini.contains("ARCH_DEST = /data/myarch"), "应使用自定义归档路径");
        assert!(ini.contains("ARCH_FILE_SIZE = 256"), "应使用自定义 file_size");
        assert!(ini.contains("ARCH_SPACE_LIMIT = 2048"), "应使用自定义 space_limit");
        assert!(ini.contains("ARCH_HANG_FLAG = 1"), "hang_flag=true 应输出 1");
        assert!(ini.contains("ARCH_COMPRESSED = 1"), "compressed=true 应输出 1");
    }
}
