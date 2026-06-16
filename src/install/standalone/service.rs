use anyhow::Result;

use crate::config::InstallConfig;
use crate::ssh::{CommandRunner, shell_quote};

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

/// 注册并启动 DMAP 和 dmserver 服务，最后修正数据目录权限。
pub async fn register_and_start(runner: &dyn CommandRunner, config: &InstallConfig) -> Result<()> {
    let (uid_out, _) = runner.exec("id -u").await.unwrap_or_default();
    let uid = String::from_utf8_lossy(&uid_out).trim().to_string();
    anyhow::ensure!(
        uid == "0",
        "服务注册需要 root 权限（当前 UID: {}）。\n\
         请以 root 身份运行，或在命令前加 sudo：\n\
         sudo dm-installer install",
        uid
    );

    register_dmap(runner, config).await?;
    register_dmserver(runner, config).await?;

    crate::ui::log_info("修正数据目录权限...");
    runner
        .exec(&format!(
            "chown -R dmdba:dinstall {}",
            shell_quote(&config.data_path)
        ))
        .await
        .map_err(|e| anyhow::anyhow!("修正数据目录权限失败: {e}"))?;

    Ok(())
}

async fn register_dmap(runner: &dyn CommandRunner, config: &InstallConfig) -> Result<()> {
    let name = DMAP_SERVICE_NAME;
    if is_active(runner, name).await {
        crate::ui::log_info(&format!("[续] DMAP 服务 {} 已在运行，跳过注册", name));
        return Ok(());
    }
    if !is_registered(runner, name).await {
        crate::ui::log_info("注册 DMAP 辅助进程服务...");
        run_installer(runner, config, &["-t", "dmap"]).await?;
    } else {
        crate::ui::log_info(&format!("[续] DMAP 服务 {} 已注册，跳过注册步骤", name));
    }
    start_service(runner, name).await
}

async fn register_dmserver(runner: &dyn CommandRunner, config: &InstallConfig) -> Result<()> {
    let name = service_name(config);
    let dm_ini = dm_ini_path(config);
    let service_bin = format!("{}/bin/{}", config.install_path, &name);

    if is_active(runner, &name).await {
        crate::ui::log_info(&format!("[续] 数据库服务 {} 已在运行，跳过注册", name));
        return Ok(());
    }

    let check_cmd = format!(
        "test -f /etc/systemd/system/{s}.service \
         || test -f /etc/init.d/{s} \
         || test -f {bin} \
         && echo registered || echo unregistered",
        s = &name,
        bin = shell_quote(&service_bin),
    );
    let (check_out, _) = runner.exec(&check_cmd).await.unwrap_or_default();
    if String::from_utf8_lossy(&check_out).trim() != "registered" {
        crate::ui::log_info("注册 dmserver 数据库服务...");
        run_installer(
            runner,
            config,
            &[
                "-t",
                "dmserver",
                "-p",
                &config.instance_name,
                "-dm_ini",
                &dm_ini,
            ],
        )
        .await?;
    } else {
        crate::ui::log_info(&format!("[续] 数据库服务 {} 已注册，跳过注册步骤", name));
    }

    crate::ui::log_info(&format!("启动 dmserver 服务 {}...", &name));
    let start_cmd = format!(
        "su - dmdba -c {} 2>&1 || systemctl start {} 2>&1",
        shell_quote(&format!("{} start", service_bin)),
        shell_quote(&name),
    );
    runner
        .exec(&start_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("启动达梦数据库服务失败: {e}"))?;
    crate::ui::log_ok(&format!("数据库服务已启动: {}", name));
    Ok(())
}

async fn is_active(runner: &dyn CommandRunner, name: &str) -> bool {
    let cmd = format!(
        "systemctl is-active --quiet {} && echo yes || echo no",
        name
    );
    runner
        .exec(&cmd)
        .await
        .map(|(out, _)| String::from_utf8_lossy(&out).trim() == "yes")
        .unwrap_or(false)
}

async fn is_registered(runner: &dyn CommandRunner, name: &str) -> bool {
    let cmd = format!(
        "test -f /etc/systemd/system/{s}.service || test -f /etc/init.d/{s} && echo yes || echo no",
        s = name,
    );
    runner
        .exec(&cmd)
        .await
        .map(|(out, _)| String::from_utf8_lossy(&out).trim() == "yes")
        .unwrap_or(false)
}

async fn run_installer(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    args: &[&str],
) -> Result<()> {
    let script = format!(
        "{}/script/root/dm_service_installer.sh",
        config.install_path
    );
    let args_str = args
        .iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ");
    let cmd = format!(
        "chmod +x {s} && bash {s} {args}",
        s = shell_quote(&script),
        args = args_str
    );
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("服务注册脚本执行失败: {e}"))?;
    Ok(())
}

async fn start_service(runner: &dyn CommandRunner, name: &str) -> Result<()> {
    runner
        .exec(&format!(
            "systemctl start {n} 2>/dev/null || service {n} start 2>/dev/null || true",
            n = shell_quote(name)
        ))
        .await
        .map_err(|e| anyhow::anyhow!("启动服务 {} 失败: {e}", name))?;
    runner
        .exec(&format!(
            "systemctl enable {} 2>/dev/null || true",
            shell_quote(name)
        ))
        .await
        .unwrap_or_default();
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
