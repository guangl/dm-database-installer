use anyhow::{Context, Result};
use std::{path::Path, process::Command};

use crate::config::InstallConfig;

/// DM 约定：dminit 以 DB_NAME=DAMENG 初始化，实例数据在 {data_path}/DAMENG/ 下。
pub fn dm_ini_path(config: &InstallConfig) -> String {
    format!("{}/DAMENG/dm.ini", config.data_path)
}

/// DMAP 辅助进程服务名固定为 DmAPService，与实例名无关。
pub const DMAP_SERVICE_NAME: &str = "DmAPService";

/// dmserver 服务名 = DmService + 实例名（由 dm_service_installer.sh 生成）。
pub fn service_name(config: &InstallConfig) -> String {
    format!("DmService{}", config.instance_name)
}

/// 按顺序注册并启动两个服务：先 DMAP（辅助进程），再 dmserver（数据库）。
/// 服务注册需要 root 权限，非 root 时提前报清晰错误。
pub fn register_and_start(config: &InstallConfig) -> Result<()> {
    ensure_root()?;
    register_and_start_dmap(config)?;
    register_and_start_dmserver(config)
}

fn register_and_start_dmap(config: &InstallConfig) -> Result<()> {
    let name = DMAP_SERVICE_NAME;
    if is_service_active(name) {
        crate::ui::log_info(&format!("[续] DMAP 服务 {} 已在运行，跳过注册", name));
        return Ok(());
    }
    if !is_service_registered(name) {
        crate::ui::log_info("注册 DMAP 辅助进程服务...");
        run_service_installer(config, &["-t", "dmap"])?;
    } else {
        crate::ui::log_info(&format!("[续] DMAP 服务 {} 已注册，跳过注册步骤", name));
    }
    start_and_enable_service(name)
}

fn register_and_start_dmserver(config: &InstallConfig) -> Result<()> {
    let name = service_name(config);
    let dm_ini = dm_ini_path(config);
    if is_service_active(&name) {
        crate::ui::log_info(&format!("[续] 数据库服务 {} 已在运行，跳过注册", name));
        return Ok(());
    }
    if !is_service_registered(&name) {
        crate::ui::log_info("注册 dmserver 数据库服务...");
        run_service_installer(config, &["-t", "dmserver", "-p", &dm_ini, "-m", "auto"])?;
    } else {
        crate::ui::log_info(&format!("[续] 数据库服务 {} 已注册，跳过注册步骤", name));
    }
    start_and_enable_service(&name)
}

/// 服务注册脚本必须以 root 运行；非 root 时提前报清晰错误，避免脚本内部报错让用户困惑。
fn ensure_root() -> Result<()> {
    // 通过 id -u 获取有效 UID；返回 "0" 即为 root。
    let output = Command::new("id")
        .arg("-u")
        .output()
        .context("执行 id -u 失败")?;
    let uid = String::from_utf8_lossy(&output.stdout);
    anyhow::ensure!(
        uid.trim() == "0",
        "服务注册需要 root 权限（当前 UID: {}）。\n\
         请以 root 身份运行，或在命令前加 sudo：\n\
         sudo dm-installer install",
        uid.trim()
    );
    Ok(())
}

fn is_service_active(name: &str) -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn is_service_registered(name: &str) -> bool {
    Path::new(&format!("/etc/systemd/system/{}.service", name)).exists()
        || Path::new(&format!("/etc/rc.d/init.d/{}", name)).exists()
        || Path::new(&format!("/etc/init.d/{}", name)).exists()
}

fn run_service_installer(config: &InstallConfig, args: &[&str]) -> Result<()> {
    let script = format!("{}/script/root/dm_service_installer.sh", config.install_path);

    anyhow::ensure!(
        Path::new(&script).exists(),
        "服务注册脚本不存在: {}（请确认 dmdbms 已正确安装）",
        script
    );

    let _ = Command::new("chmod").arg("+x").arg(&script).status();

    let status = Command::new("bash")
        .arg(&script)
        .args(args)
        .status()
        .with_context(|| format!("执行服务注册脚本失败: {}", script))?;

    anyhow::ensure!(
        status.success(),
        "服务注册脚本返回非零退出码: {:?}",
        status.code()
    );
    Ok(())
}

fn start_and_enable_service(name: &str) -> Result<()> {
    let has_systemctl = Command::new("systemctl")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if has_systemctl {
        Command::new("systemctl")
            .args(["start", name])
            .status()
            .with_context(|| format!("systemctl start {} 失败", name))?;

        // enable 失败不致命（容器环境可能不支持）
        if !Command::new("systemctl")
            .args(["enable", name])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            tracing::warn!("systemctl enable {} 失败（容器/非 systemd 环境？），服务已启动但未设置开机自启", name);
            crate::ui::log_warn(&format!("服务 {} 已启动（注意：开机自启设置失败，可能为容器环境）", name));
            return Ok(());
        }
    } else {
        // 降级：init.d 脚本
        let init_script = format!("/etc/init.d/{}", name);
        if Path::new(&init_script).exists() {
            Command::new(&init_script)
                .arg("start")
                .status()
                .with_context(|| format!("启动服务 {} 失败", name))?;
        } else {
            Command::new("service")
                .args([name, "start"])
                .status()
                .with_context(|| format!("service {} start 失败", name))?;
        }
    }

    crate::ui::log_ok(&format!("服务注册完成: {}", name));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(instance: &str, data: &str, install: &str) -> InstallConfig {
        InstallConfig {
            install_path: install.to_string(),
            data_path: data.to_string(),
            instance_name: instance.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_dm_ini_path() {
        let c = cfg("DMSERVER", "/opt/dm/data", "/opt/dm");
        assert_eq!(dm_ini_path(&c), "/opt/dm/data/DAMENG/dm.ini");
    }

    #[test]
    fn test_service_name() {
        let c = cfg("DMSERVER", "/opt/dm/data", "/opt/dm");
        assert_eq!(service_name(&c), "DmServiceDMSERVER");
    }

    #[test]
    fn test_service_name_custom_instance() {
        let c = cfg("MYDB", "/opt/dm/data", "/opt/dm");
        assert_eq!(service_name(&c), "DmServiceMYDB");
    }
}
