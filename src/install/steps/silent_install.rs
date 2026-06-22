use anyhow::{Context, Result};
use std::{path::Path, process::Command};
use tempfile::TempDir;

/// 执行 DMInstall.bin 静默安装（-q 响应文件模式，与 install.sh 一致），
/// 不在此处初始化数据库（INIT_DB=N，数据库初始化由调用方单独执行 dminit）。
/// 注意：不要加 -i，会让 DMInstall.bin 回退到交互模式，重新弹出语言选择提示。
pub fn install_from_bin(bin_path: &Path, install_path: &str) -> Result<()> {
    let tmp = TempDir::new().context("创建临时目录失败")?;
    let response_xml = tmp.path().join("dm_install.xml");
    std::fs::write(&response_xml, build_response_xml(install_path))
        .context("写入响应文件失败")?;

    let status = Command::new(bin_path)
        .arg("-q")
        .arg(&response_xml)
        .status()
        .with_context(|| format!("执行 {} 失败", bin_path.display()))?;
    anyhow::ensure!(
        status.success(),
        "DMInstall.bin 静默安装返回非零退出码: {:?}",
        status.code()
    );

    anyhow::ensure!(
        Path::new(install_path).join("bin/dminit").exists(),
        "DMInstall.bin 执行完成但未在安装目录找到 bin/dminit（安装路径: {}）",
        install_path
    );

    Ok(())
}

fn build_response_xml(install_path: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<DATABASE>
    <LANGUAGE>ZH</LANGUAGE>
    <INSTALL_TYPE>0</INSTALL_TYPE>
    <INSTALL_PATH>{install_path}</INSTALL_PATH>
    <INIT_DB>N</INIT_DB>
</DATABASE>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_from_bin_fails_on_nonexistent_file() {
        let result = install_from_bin(Path::new("/nonexistent/DMInstall.bin"), "/tmp/dm_test");
        assert!(result.is_err(), "不存在的文件应返回错误");
    }

    #[test]
    fn test_install_from_bin_fails_when_bin_not_executable_installer() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin_path = tmp.path().join("DMInstall.bin");
        // 一个能成功执行但不安装任何文件的假 "installer"（/bin/true）
        std::fs::write(&bin_path, "#!/bin/true\n").unwrap();
        std::fs::set_permissions(
            &bin_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        let install_dir = tmp.path().join("install");
        let result = install_from_bin(&bin_path, install_dir.to_str().unwrap());
        assert!(result.is_err(), "未生成 bin/dminit 应返回错误");
    }

    #[test]
    fn test_build_response_xml_contains_install_path_and_no_init() {
        let xml = build_response_xml("/opt/dmdbms");
        assert!(xml.contains("<INSTALL_PATH>/opt/dmdbms</INSTALL_PATH>"));
        assert!(xml.contains("<INIT_DB>N</INIT_DB>"));
    }
}
