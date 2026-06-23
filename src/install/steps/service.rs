use anyhow::Result;

use crate::config::InstallConfig;
use crate::ssh::{CommandRunner, shell_quote};

/// 等待数据库就绪：轮询 disql 连接，最多 120 秒（60 次 × 2 秒）。
pub async fn wait_ready(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Result<()> {
    let disql = format!("{}/bin/disql", config.install_path);
    let log_file = format!(
        "{}/DAMENG/dm_{}.log",
        config.data_path, config.instance_name
    );
    let conn = format!("SYSDBA/{}@localhost:{}", sysdba_pwd, config.port);
    let inner_cmd = format!(
        "printf 'exit;\\n' | {} {} >/dev/null 2>&1",
        shell_quote(&disql),
        shell_quote(&conn),
    );
    let check_cmd = format!(
        "su - dmdba -c {} && echo ok || echo fail",
        shell_quote(&inner_cmd),
    );
    let alive_cmd = "pgrep -u dmdba dmserver >/dev/null 2>&1 && echo alive || echo dead";

    crate::ui::log_info("等待数据库就绪...");
    for attempt in 1..=60u32 {
        let alive = runner
            .exec(alive_cmd)
            .await
            .map(|(out, _)| String::from_utf8_lossy(&out).trim() == "alive")
            .unwrap_or(false);
        if !alive {
            anyhow::bail!("dmserver 进程已退出，请检查日志: {}", log_file);
        }
        let ready = runner
            .exec(&check_cmd)
            .await
            .map(|(out, _)| String::from_utf8_lossy(&out).trim() == "ok")
            .unwrap_or(false);
        if ready {
            return Ok(());
        }
        if attempt < 60 {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }
    anyhow::bail!("数据库未在 120 秒内就绪，请检查日志: {}", log_file)
}

/// 等待 dmserver 进程存活（不要求能登录），用于主备集群中 standby 节点：
/// standby 启动后处于 MOUNT 状态等待守护接管同步，此时未必能以 SYSDBA 正常登录，
/// 因此只确认进程仍在运行，而不像 `wait_ready` 那样要求 disql 连接成功。
pub async fn wait_process_alive(runner: &dyn CommandRunner, timeout_secs: u32) -> Result<()> {
    let alive_cmd = "pgrep -u dmdba dmserver >/dev/null 2>&1 && echo alive || echo dead";
    let attempts = timeout_secs.div_ceil(2).max(1);
    for attempt in 1..=attempts {
        let alive = runner
            .exec(alive_cmd)
            .await
            .map(|(out, _)| String::from_utf8_lossy(&out).trim() == "alive")
            .unwrap_or(false);
        if alive {
            return Ok(());
        }
        if attempt < attempts {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }
    anyhow::bail!("dmserver 进程未在 {} 秒内启动", timeout_secs)
}

/// 查询达梦数据库版本号（执行 SELECT id_code）。
pub async fn query_dm_version(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    sysdba_pwd: &str,
) -> Option<String> {
    let disql = format!("{}/bin/disql", config.install_path);
    let conn = format!("SYSDBA/{}@localhost:{}", sysdba_pwd, config.port);
    let inner_cmd = format!(
        "printf 'SELECT id_code;\\nexit;\\n' | {} {}",
        shell_quote(&disql),
        shell_quote(&conn),
    );
    let cmd = format!("su - dmdba -c {} 2>/dev/null", shell_quote(&inner_cmd));
    let (out, _) = runner.exec(&cmd).await.ok()?;
    parse_dm_version_local(&String::from_utf8_lossy(&out))
}

/// 从 disql SELECT id_code 输出中提取版本字符串。
/// 格式：分隔线 "---" 之后第一个非空行的最后字段（与 install.sh awk 逻辑一致）。
fn parse_dm_version_local(output: &str) -> Option<String> {
    let mut past_sep = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("---") {
            past_sep = true;
            continue;
        }
        if past_sep && !trimmed.is_empty() {
            return trimmed.split_whitespace().last().map(str::to_string);
        }
    }
    None
}

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

/// dmwatcher 服务名 = DmWatcherService + 实例名（与 dmserver 同样以实例名区分）。
pub fn watcher_service_name(instance_name: &str) -> String {
    format!("DmWatcherService{instance_name}")
}

/// dmmonitor 服务名 = DmMonitorService + 实例名。
pub fn monitor_service_name(instance_name: &str) -> String {
    format!("DmMonitorService{instance_name}")
}

/// 注册并启动 DMAP 和 dmserver 服务（正常模式），最后修正数据目录权限。
pub async fn register_and_start(runner: &dyn CommandRunner, config: &InstallConfig) -> Result<()> {
    register_and_start_inner(runner, config, None).await
}

/// 注册并启动 DMAP 和 dmserver 服务，dmserver 以 Mount 模式启动
/// （`dm_service_installer.sh -m mount`，主备集群官方要求"一定要以 Mount 方式启动"）。
/// 服务注册后由 systemd 管理生命周期，之后每次启动/重启都会保持 Mount 模式，
/// 交由已在运行的 dmwatcher 负责将其切换为 Open 状态。
pub async fn register_and_start_mount(runner: &dyn CommandRunner, config: &InstallConfig) -> Result<()> {
    register_and_start_inner(runner, config, Some("mount")).await
}

async fn register_and_start_inner(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    mode: Option<&str>,
) -> Result<()> {
    let (uid_out, _) = runner.exec("id -u").await.unwrap_or_default();
    let uid = String::from_utf8_lossy(&uid_out).trim().to_string();
    anyhow::ensure!(
        uid == "0",
        "服务注册需要 root 权限（当前 UID: {}）。\n\
         请以 root 身份运行，或在命令前加 sudo：\n\
         sudo dm_installer install",
        uid
    );

    register_dmap(runner, config).await?;
    register_dmserver(runner, config, mode).await?;

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
        run_installer(runner, &config.install_path, &["-t", "dmap"]).await?;
    } else {
        crate::ui::log_info(&format!("[续] DMAP 服务 {} 已注册，跳过注册步骤", name));
    }
    start_service(runner, name).await
}

async fn register_dmserver(
    runner: &dyn CommandRunner,
    config: &InstallConfig,
    mode: Option<&str>,
) -> Result<()> {
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
        crate::ui::log_info(&format!(
            "注册 dmserver 数据库服务{}...",
            mode.map(|m| format!("（{m} 模式）")).unwrap_or_default()
        ));
        let mut args = vec!["-t", "dmserver", "-p", &config.instance_name, "-dm_ini", &dm_ini];
        if let Some(m) = mode {
            args.push("-m");
            args.push(m);
        }
        run_installer(runner, &config.install_path, &args).await?;
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

/// 注册并启动 dmwatcher 守护进程服务（`-t dmwatcher -watcher_ini <path>`）。
/// 需要 root 权限；用 `instance_name` 区分同一控制机管理的多个节点对应的服务名。
pub async fn register_and_start_watcher(
    runner: &dyn CommandRunner,
    install_path: &str,
    instance_name: &str,
    watcher_ini: &str,
) -> Result<()> {
    let name = watcher_service_name(instance_name);
    if is_active(runner, &name).await {
        crate::ui::log_info(&format!("[续] dmwatcher 服务 {} 已在运行，跳过注册", name));
        return Ok(());
    }
    if !is_registered(runner, &name).await {
        crate::ui::log_info("注册 dmwatcher 守护进程服务...");
        run_installer(
            runner,
            install_path,
            &["-t", "dmwatcher", "-p", instance_name, "-watcher_ini", watcher_ini],
        )
        .await?;
    } else {
        crate::ui::log_info(&format!("[续] dmwatcher 服务 {} 已注册，跳过注册步骤", name));
    }
    start_service(runner, &name).await
}

/// 注册并启动 dmmonitor 确认监视器服务（`-t dmmonitor -monitor_ini <path>`）。
pub async fn register_and_start_monitor(
    runner: &dyn CommandRunner,
    install_path: &str,
    instance_name: &str,
    monitor_ini: &str,
) -> Result<()> {
    let name = monitor_service_name(instance_name);
    if is_active(runner, &name).await {
        crate::ui::log_info(&format!("[续] dmmonitor 服务 {} 已在运行，跳过注册", name));
        return Ok(());
    }
    if !is_registered(runner, &name).await {
        crate::ui::log_info("注册 dmmonitor 监视器服务...");
        run_installer(
            runner,
            install_path,
            &["-t", "dmmonitor", "-p", instance_name, "-monitor_ini", monitor_ini],
        )
        .await?;
    } else {
        crate::ui::log_info(&format!("[续] dmmonitor 服务 {} 已注册，跳过注册步骤", name));
    }
    start_service(runner, &name).await
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

async fn run_installer(runner: &dyn CommandRunner, install_path: &str, args: &[&str]) -> Result<()> {
    let script = format!("{install_path}/script/root/dm_service_installer.sh");
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

/// 重启 dmserver 服务（参数调整等场景需要重启才能生效）。
pub async fn restart_dmserver(runner: &dyn CommandRunner, config: &InstallConfig) -> Result<()> {
    let name = service_name(config);
    runner
        .exec(&format!(
            "systemctl restart {n} 2>/dev/null || service {n} restart 2>/dev/null",
            n = shell_quote(&name)
        ))
        .await
        .map_err(|e| anyhow::anyhow!("重启服务 {} 失败: {e}", name))?;
    crate::ui::log_ok(&format!("服务已重启: {}", name));
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

    #[test]
    fn test_watcher_service_name() {
        assert_eq!(watcher_service_name("DM01"), "DmWatcherServiceDM01");
    }

    #[test]
    fn test_monitor_service_name() {
        assert_eq!(monitor_service_name("DM01"), "DmMonitorServiceDM01");
    }

    fn root_runner(responses: Vec<(String, u32, Vec<u8>)>) -> crate::ssh::MockRunner {
        let mut all = vec![("id -u".to_string(), 0, b"0\n".to_vec())];
        all.extend(responses);
        crate::ssh::MockRunner::new(all)
    }

    #[tokio::test]
    async fn test_register_and_start_mount_passes_m_mount_flag() {
        let runner = root_runner(vec![]);
        let c = cfg("DM01", "/opt/dm/data", "/opt/dm");
        register_and_start_mount(&runner, &c).await.unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter()
                .any(|cmd| cmd.contains("'-t' 'dmserver'") && cmd.contains("'-m' 'mount'")),
            "应以 -t dmserver -m mount 注册服务: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_register_and_start_does_not_pass_mode_flag() {
        let runner = root_runner(vec![]);
        let c = cfg("DM01", "/opt/dm/data", "/opt/dm");
        register_and_start(&runner, &c).await.unwrap();
        let log = runner.exec_log();
        assert!(
            !log.iter().any(|cmd| cmd.contains("'-m'")),
            "单机注册不应带 -m 参数: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_register_and_start_watcher_uses_watcher_ini() {
        let runner = root_runner(vec![]);
        register_and_start_watcher(&runner, "/opt/dm", "DM01", "/opt/dm/data/DAMENG/dmwatcher.ini")
            .await
            .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|cmd| cmd.contains("'-t' 'dmwatcher'")
                && cmd.contains("'-watcher_ini' '/opt/dm/data/DAMENG/dmwatcher.ini'")),
            "应注册 dmwatcher 服务并指定 watcher_ini: {:?}",
            log
        );
    }

    #[tokio::test]
    async fn test_register_and_start_monitor_uses_monitor_ini() {
        let runner = root_runner(vec![]);
        register_and_start_monitor(&runner, "/opt/dm", "DM01", "/opt/dm/data/dmmonitor.ini")
            .await
            .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|cmd| cmd.contains("'-t' 'dmmonitor'")
                && cmd.contains("'-monitor_ini' '/opt/dm/data/dmmonitor.ini'")),
            "应注册 dmmonitor 服务并指定 monitor_ini: {:?}",
            log
        );
    }
}
