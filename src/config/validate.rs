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
        writeln!(file, "port = not_a_number").unwrap();
        let args = ValidateArgs { config: file.path().to_path_buf() };
        assert!(run(&args).is_err());
    }

    #[test]
    fn test_nonexistent_file_fails() {
        let args = ValidateArgs { config: "/nonexistent/path/dm.toml".into() };
        assert!(run(&args).is_err());
    }
}
