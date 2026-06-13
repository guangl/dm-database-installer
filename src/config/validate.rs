use anyhow::Result;

use crate::cli::ValidateArgs;

/// 运行 validate 子命令：读取并解析 TOML 配置文件，验证字段语义合法性。
/// 共用 config::load_and_validate() 三步链，不执行安装。
pub fn run(args: &ValidateArgs) -> Result<()> {
    super::load_and_validate(&args.config)?;
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
        writeln!(file, "port = 5236\n").unwrap();
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
        let cfg = InstallConfig::default();
        assert_eq!(cfg.install_path, "/home/dmdba/dmdbms");
        assert_eq!(cfg.data_path, "/home/dmdba/dmdbms/data");
        assert_eq!(cfg.instance_name, "DMSERVER");
        assert_eq!(cfg.port, 5236);
        assert_eq!(cfg.page_size, 32);
        assert_eq!(cfg.charset, 1);
        assert!(cfg.case_sensitive);
        assert_eq!(cfg.extent_size, 32);
    }

    #[test]
    fn test_install_config_partial_toml() {
        // 仅覆盖 port，其余字段应保持 D-07 默认值
        let toml_str = "port = 5237\n";
        let cfg: InstallConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.port, 5237, "port 应被覆盖为 5237");
        assert_eq!(
            cfg.install_path, "/home/dmdba/dmdbms",
            "install_path 未指定时应保持默认值"
        );
    }

    #[test]
    fn test_semantic_invalid_fixture_rejected() {
        // 语义非法 fixture（page_size=12）应被 validate 子命令拒绝
        let args = ValidateArgs {
            config: "tests/fixtures/semantic_invalid.toml".into(),
        };
        let err = run(&args).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(
            msg.contains("page_size 无效: 12"),
            "错误链应含 'page_size 无效: 12'，实际: {msg}"
        );
    }
}
