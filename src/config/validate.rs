use anyhow::Result;

use crate::cli::ValidateArgs;

/// 运行 validate 子命令：加载并验证 config.toml + 特有配置文件，不执行安装。
pub fn run(args: &ValidateArgs) -> Result<()> {
    let (common_path, path_display) = match &args.config {
        Some(path) => (path.as_path(), path.display().to_string()),
        None => (
            std::path::Path::new(super::CONFIG_FILE),
            super::CONFIG_FILE.to_string(),
        ),
    };
    let common = super::load_common_config(common_path)?;
    let specific_file = common.install_type.specific_config_file();
    let dir = common_path.parent().unwrap_or(std::path::Path::new("."));
    let specific_path = dir.join(specific_file);
    match common.install_type {
        super::InstallType::Standalone => {
            super::load_standalone_specific(&specific_path)?;
        }
        install_type => {
            super::cluster::load_cluster_specific(&specific_path, install_type)?;
        }
    }
    println!("配置文件合法: {} + {}", path_display, specific_file);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::InstallConfig;
    use std::io::Write;
    use tempfile::NamedTempFile;

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
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "[instance]\nport = 5237").unwrap();
        let cfg = super::super::load_standalone_specific(file.path()).unwrap();
        assert_eq!(cfg.port, 5237);
        assert_eq!(cfg.install_path, "/home/dmdba/dmdbms");
    }
}
