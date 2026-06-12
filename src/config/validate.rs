use anyhow::{Context, Result};

use crate::cli::ValidateArgs;

use super::InstallConfig;

/// 运行 validate 子命令：读取并解析 TOML 配置文件，验证字段合法性。
/// Phase 1 占位：仅验证 TOML 语法和已定义字段，不执行安装。
pub fn run(args: &ValidateArgs) -> Result<()> {
    let content = std::fs::read_to_string(&args.config)
        .with_context(|| format!("无法读取配置文件: {}", args.config.display()))?;

    toml::from_str::<InstallConfig>(&content)
        .with_context(|| "配置文件解析失败")?;

    println!("配置文件合法: {}", args.config.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use crate::config::InstallConfig;

    #[test]
    fn test_valid_toml_passes() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"port = 5236"#).unwrap();
        let args = ValidateArgs { config: file.path().to_path_buf() };
        assert!(run(&args).is_ok());
    }

    #[test]
    fn test_invalid_toml_fails() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, r#"port = "not_a_number""#).unwrap();
        let args = ValidateArgs { config: file.path().to_path_buf() };
        let err = run(&args).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("配置文件解析失败"),
            "错误链应包含'配置文件解析失败'，实际: {msg}"
        );
    }

    #[test]
    fn test_missing_file_fails() {
        let args = ValidateArgs { config: "/nonexistent/path/dm.toml".into() };
        let err = run(&args).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("无法读取配置文件"),
            "错误链应包含'无法读取配置文件'，实际: {msg}"
        );
    }

    #[test]
    fn test_install_config_defaults() {
        // 验证 InstallConfig::default() D-07 规定的默认值
        let cfg = InstallConfig::default();
        assert_eq!(cfg.install_path, "/opt/dmdbms");
        assert_eq!(cfg.data_path, "/opt/dmdbms/data");
        assert_eq!(cfg.instance_name, "DMSERVER");
        assert_eq!(cfg.port, 5236);
        assert_eq!(cfg.page_size, 8);
        assert_eq!(cfg.charset, 0);
        assert!(cfg.case_sensitive);
        assert_eq!(cfg.extent_size, 16);
    }

    #[test]
    fn test_install_config_partial_toml() {
        // 仅覆盖 port，其余字段应保持 D-07 默认值
        let toml_str = "port = 5237\n";
        let cfg: InstallConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.port, 5237, "port 应被覆盖为 5237");
        assert_eq!(
            cfg.install_path, "/opt/dmdbms",
            "install_path 未指定时应保持默认值"
        );
    }
}
