use anyhow::{Context, Result};
use std::path::Path;

use crate::common::ssh::CommandRunner;
use crate::cluster::templates::{
    generate_dm_ini_cluster_suffix, generate_dmarch_ini, generate_dmmal_ini, generate_dmwatcher_ini,
};
use crate::config::cluster::{ArchiveConfig, DminitConfig, MalConfig, NodeConfig, NodeRole, SqlLogConfig, WatcherConfig};
use crate::config::InstallConfig;
use crate::standalone::silent_install::generate_install_xml;

use crate::common::shell_quote;

/// 构建 dminit 命令行参数列表（等号两侧无空格，防止 Pitfall 2）。
/// 路径和实例名经 shell_quote 转义（CR-04 防注入）。
pub fn build_dminit_args(node: &NodeConfig, dminit: &DminitConfig) -> Vec<String> {
    vec![
        format!("{}/bin/dminit", shell_quote(&dminit.install_path)),
        format!("PATH={}", shell_quote(&dminit.data_path)),
        format!("INSTANCE_NAME={}", shell_quote(&node.instance_name)),
        format!("PORT_NUM={}", dminit.port),
        format!("PAGE_SIZE={}", dminit.page_size),
        format!("CHARSET={}", dminit.charset),
        format!("CASE_SENSITIVE={}", if dminit.case_sensitive { 1 } else { 0 }),
        format!("EXTENT_SIZE={}", dminit.extent_size),
    ]
}

/// 上传安装包 + XML response file，执行远端静默安装。
pub async fn upload_installer_and_install(
    node: &NodeConfig,
    dminit: &DminitConfig,
    package_path: &Path,
    runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("[node:{:?}][1/6] 生成 XML response file", node.role);
    let install_config = node_to_install_config(node, dminit);
    let language = detect_node_language(runner).await;
    let xml_file = generate_install_xml(&install_config, language).context("生成 XML response file 失败")?;
    let xml_content = std::fs::read_to_string(xml_file.path()).context("读取 XML 临时文件失败")?;
    let remote_xml = format!("/tmp/cluster_install_{}.xml", node.instance_name);
    runner
        .sftp_write(&remote_xml, xml_content.as_bytes())
        .await
        .context("SFTP 上传 XML response file 失败")?;
    tracing::info!("[node:{:?}][2/6] 推送安装包", node.role);
    let bytes = tokio::fs::read(package_path)
        .await
        .with_context(|| format!("无法读取安装包 {}", package_path.display()))?;
    let remote_bin_path = format!("/tmp/dm_installer_{}.bin", node.instance_name);
    runner
        .sftp_write(&remote_bin_path, &bytes)
        .await
        .context("SFTP 上传安装包失败")?;
    runner
        .exec(&format!("chmod +x {}", shell_quote(&remote_bin_path)))
        .await
        .map_err(|e| anyhow::anyhow!("chmod 安装包失败: {}", e))?;
    let install_cmd = format!("{} -q {}", shell_quote(&remote_bin_path), shell_quote(&remote_xml));
    let (stdout, exit_code) = runner
        .exec(&install_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("DMInstall.bin 执行失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "DMInstall.bin 失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    Ok(())
}

/// 将 NodeConfig + DminitConfig 映射为 InstallConfig（用于 XML 生成）。
fn node_to_install_config(node: &NodeConfig, dminit: &DminitConfig) -> InstallConfig {
    InstallConfig {
        install_path: dminit.install_path.clone(),
        data_path: dminit.data_path.clone(),
        instance_name: node.instance_name.clone(),
        port: dminit.port,
        page_size: dminit.page_size,
        charset: dminit.charset,
        case_sensitive: dminit.case_sensitive,
        extent_size: dminit.extent_size,
        archive: Default::default(),
        ssh_target: None,
    }
}

/// 远端执行 dminit 初始化数据库。
pub async fn run_dminit_remote(node: &NodeConfig, dminit: &DminitConfig, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[node:{:?}][3/6] 执行 dminit", node.role);
    let cmd = build_dminit_args(node, dminit).join(" ");
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("dminit 执行失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "dminit 失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    Ok(())
}

async fn detect_node_language(runner: &dyn CommandRunner) -> &'static str {
    let lang = runner
        .exec("echo $LANG")
        .await
        .map(|(bytes, _)| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let language = if lang.trim().to_lowercase().contains("zh") { "ZH" } else { "EN" };
    tracing::debug!("节点 $LANG={:?} -> 安装语言: {}", lang.trim(), language);
    language
}

/// 计算远端配置文件目标路径。
fn target_path(node: &NodeConfig, dminit: &DminitConfig, filename: &str) -> String {
    format!("{}/{}/{}", dminit.data_path, node.instance_name, filename)
}

/// 分发 4 个 INI 配置文件到远端节点。
pub async fn distribute_configs(
    node: &NodeConfig,
    dminit: &DminitConfig,
    all_nodes: &[NodeConfig],
    oguid: u32,
    archive: &ArchiveConfig,
    mal: &MalConfig,
    watcher: &WatcherConfig,
    runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("[node:{:?}][4/6] 分发配置文件", node.role);
    let peer = all_nodes
        .iter()
        .find(|n| n.instance_name != node.instance_name)
        .context("找不到对端节点")?;
    let dm_ini_suffix = generate_dm_ini_cluster_suffix();
    let dmmal_ini = generate_dmmal_ini(all_nodes, dminit, mal);
    let dmarch_ini = generate_dmarch_ini(node, dminit, &peer.instance_name, archive);
    let dmwatcher_ini = generate_dmwatcher_ini(node, dminit, oguid, watcher);
    let dm_ini_path = target_path(node, dminit, "dm.ini.cluster_suffix");
    tracing::debug!("[node:{:?}] 上传 dm.ini.cluster_suffix ({} bytes)", node.role, dm_ini_suffix.len());
    runner
        .sftp_write(&dm_ini_path, dm_ini_suffix.as_bytes())
        .await
        .context("SFTP 上传 dm.ini.cluster_suffix 失败")?;

    let dmmal_path = target_path(node, dminit, "dmmal.ini");
    tracing::debug!("[node:{:?}] 上传 dmmal.ini ({} bytes)", node.role, dmmal_ini.len());
    runner
        .sftp_write(&dmmal_path, dmmal_ini.as_bytes())
        .await
        .context("SFTP 上传 dmmal.ini 失败")?;

    let dmarch_path = target_path(node, dminit, "dmarch.ini");
    tracing::debug!("[node:{:?}] 上传 dmarch.ini ({} bytes)", node.role, dmarch_ini.len());
    runner
        .sftp_write(&dmarch_path, dmarch_ini.as_bytes())
        .await
        .context("SFTP 上传 dmarch.ini 失败")?;

    let dmwatcher_path = target_path(node, dminit, "dmwatcher.ini");
    tracing::debug!("[node:{:?}] 上传 dmwatcher.ini ({} bytes)", node.role, dmwatcher_ini.len());
    runner
        .sftp_write(&dmwatcher_path, dmwatcher_ini.as_bytes())
        .await
        .context("SFTP 上传 dmwatcher.ini 失败")?;
    let merge_cmd = format!(
        "cat {} >> {}",
        shell_quote(&target_path(node, dminit, "dm.ini.cluster_suffix")),
        shell_quote(&target_path(node, dminit, "dm.ini"))
    );
    runner
        .exec(&merge_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("合并 dm.ini 失败: {}", e))?;

    Ok(())
}

/// 以正常模式（非 mount）启动 dmserver（用于 dminit 后首次启动，初始化内部结构）。
///
/// DM 文档约束：新初始化的库首次启动不允许使用 mount 方式，必须先正常启动并退出。
pub async fn start_dmserver_normal(node: &NodeConfig, dminit: &DminitConfig, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[node:{:?}] 首次正常启动 dmserver（初始化内部结构）", node.role);
    let install_path = shell_quote(&dminit.install_path);
    let data_path = shell_quote(&dminit.data_path);
    let instance_name = shell_quote(&node.instance_name);
    let log_path = shell_quote(&format!("/tmp/dmserver_init_{}.log", node.instance_name));
    let cmd = format!(
        "nohup {install_path}/bin/dmserver {data_path}/{instance_name}/dm.ini > {log_path} 2>&1 &"
    );
    runner.exec(&cmd).await.map_err(|e| anyhow::anyhow!("正常模式启动 dmserver 失败: {}", e))?;
    Ok(())
}

/// 通过 disql 执行 shutdown immediate 停止数据库实例。
pub async fn stop_dmserver(node: &NodeConfig, dminit: &DminitConfig, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[node:{:?}] 停止 dmserver", node.role);
    let cmd = format!(
        "echo 'shutdown immediate;' | {}/bin/disql SYSDBA/{}@localhost:{}",
        shell_quote(&dminit.install_path),
        shell_quote(&dminit.sysdba_password),
        dminit.port,
    );
    runner.exec(&cmd).await.map_err(|e| anyhow::anyhow!("停止 dmserver 失败: {}", e))?;
    Ok(())
}

/// 执行 dmrman 脱机全量备份，备份集保存到 backup_dir。
///
/// 调用前数据库必须已停止（脱机备份）。
pub async fn run_dmrman_backup(node: &NodeConfig, dminit: &DminitConfig, backup_dir: &str, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[node:{:?}] dmrman 脱机全量备份 -> {}", node.role, backup_dir);
    let dm_ini = format!("{}/{}/dm.ini", dminit.data_path, node.instance_name);
    let ctlstmt = format!("BACKUP DATABASE '{}' FULL TO dm_full_backup BACKUPSET '{}'", dm_ini, backup_dir);
    run_dmrman_ctlstmt(runner, &shell_quote(&dminit.install_path), &ctlstmt, "BACKUP").await
}

/// 从远端节点下载备份目录中的所有文件。返回 (相对路径, 内容) 列表。
pub async fn download_backup_files(runner: &dyn CommandRunner, backup_dir: &str) -> Result<Vec<(String, Vec<u8>)>> {
    let (stdout, _) = runner.exec(&format!("find {} -type f", shell_quote(backup_dir)))
        .await.map_err(|e| anyhow::anyhow!("列出备份文件失败: {}", e))?;
    let mut files = Vec::new();
    for path in String::from_utf8_lossy(&stdout).lines().filter(|l| !l.trim().is_empty()) {
        let bytes = runner.sftp_read(path).await
            .map_err(|e| anyhow::anyhow!("下载备份文件 {} 失败: {}", path, e))?;
        let rel = path.strip_prefix(backup_dir).unwrap_or(path).trim_start_matches('/');
        files.push((rel.to_string(), bytes));
    }
    Ok(files)
}

/// 上传备份文件列表到远端节点的 backup_dir 目录。
pub async fn upload_backup_files(runner: &dyn CommandRunner, backup_dir: &str, files: &[(String, Vec<u8>)]) -> Result<()> {
    runner.exec(&format!("mkdir -p {}", shell_quote(backup_dir)))
        .await.map_err(|e| anyhow::anyhow!("创建备份目录失败: {}", e))?;
    for (rel_path, bytes) in files {
        let remote = format!("{}/{}", backup_dir, rel_path);
        runner.sftp_write(&remote, bytes).await
            .map_err(|e| anyhow::anyhow!("上传备份文件 {} 失败: {}", remote, e))?;
    }
    Ok(())
}

/// 在备节点执行 dmrman 三步还原：RESTORE → RECOVER FOR STANDBY → UPDATE DB_MAGIC。
///
/// DM 文档要求：备节点数据必须通过备份还原同步，禁止独立初始化（永久魔数不匹配会导致主节点拒绝备节点）。
pub async fn run_dmrman_restore(node: &NodeConfig, dminit: &DminitConfig, backup_dir: &str, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[node:{:?}] dmrman 三步还原（RESTORE/RECOVER/UPDATE DB_MAGIC）", node.role);
    let dm_ini = format!("{}/{}/dm.ini", dminit.data_path, node.instance_name);
    let install = shell_quote(&dminit.install_path);
    run_dmrman_ctlstmt(runner, &install, &format!("RESTORE DATABASE '{}' FROM BACKUPSET '{}'", dm_ini, backup_dir), "RESTORE").await?;
    run_dmrman_ctlstmt(runner, &install, &format!("RECOVER DATABASE '{}' FOR STANDBY", dm_ini), "RECOVER FOR STANDBY").await?;
    run_dmrman_ctlstmt(runner, &install, &format!("RECOVER DATABASE '{}' UPDATE DB_MAGIC", dm_ini), "UPDATE DB_MAGIC").await
}

async fn run_dmrman_ctlstmt(runner: &dyn CommandRunner, install: &str, ctlstmt: &str, desc: &str) -> Result<()> {
    let cmd = format!("{}/bin/dmrman CTLSTMT={}", install, shell_quote(ctlstmt));
    let (stdout, exit_code) = runner.exec(&cmd).await
        .map_err(|e| anyhow::anyhow!("dmrman {} 执行失败: {}", desc, e))?;
    anyhow::ensure!(exit_code == 0, "dmrman {} 失败 (exit {}): {}", desc, exit_code, String::from_utf8_lossy(&stdout));
    Ok(())
}

/// 以 mount 模式启动 dmserver（后台 nohup，Pitfall 4）。
pub async fn start_dmserver_mount(node: &NodeConfig, dminit: &DminitConfig, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[node:{:?}][5/6] mount 模式启动 dmserver", node.role);
    let install_path = shell_quote(&dminit.install_path);
    let data_path = shell_quote(&dminit.data_path);
    let instance_name = shell_quote(&node.instance_name);
    let log_path = shell_quote(&format!("/tmp/dmserver_{}.log", node.instance_name));
    let cmd = format!(
        "nohup {install_path}/bin/dmserver {data_path}/{instance_name}/dm.ini mount > {log_path} 2>&1 &"
    );
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("启动 dmserver 失败: {}", e))?;
    Ok(())
}

/// 通过 disql 配置数据库角色（primary 或 standby）。
pub async fn configure_database_role(
    node: &NodeConfig,
    dminit: &DminitConfig,
    role: NodeRole,
    oguid: u32,
    runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("[node:{:?}] 配置数据库角色: {:?} (oguid={})", node.role, role, oguid);
    let role_sql = match role {
        NodeRole::Primary => "alter database primary;",
        NodeRole::Standby => "alter database standby;",
        NodeRole::Monitor => anyhow::bail!("monitor 节点不支持配置数据库角色"),
    };
    let sql_block = format!(
        "SP_SET_PARA_VALUE(1,'ALTER_MODE_STATUS',1);sp_set_oguid({oguid});{role_sql}SP_SET_PARA_VALUE(1,'ALTER_MODE_STATUS',0);"
    );
    let cmd = format!(
        "echo \"{}\" | {}/bin/disql SYSDBA/{}@localhost:{}",
        sql_block,
        shell_quote(&dminit.install_path),
        shell_quote(&dminit.sysdba_password),
        dminit.port
    );
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("disql 执行失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "disql 配置角色失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    Ok(())
}

/// 通过 dm_service_installer.sh 注册 dmwatcher 服务，再 systemctl start/enable。
pub async fn register_and_start_dmwatcher_service(
    node: &NodeConfig,
    dminit: &DminitConfig,
    runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("[node:{:?}][6/6] 注册并启动 dmwatcher 服务", node.role);
    let script = format!("{}/script/root/dm_service_installer.sh", dminit.install_path);
    let watcher_ini = format!("{}/{}/dmwatcher.ini", dminit.data_path, node.instance_name);
    let register_cmd = format!(
        "bash {} -t dmwatcher -watcher_ini {}",
        shell_quote(&script),
        shell_quote(&watcher_ini),
    );
    let (stdout, exit_code) = runner
        .exec(&register_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("执行 dm_service_installer.sh 失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "dmwatcher 服务注册失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    start_and_enable_remote_service("DmWatcherService", runner).await
}

/// 通过 dm_service_installer.sh 注册 dmmonitor 服务，再 systemctl start/enable。
///
/// monitor_ini_path：已上传到远端节点的 dmmonitor.ini 路径。
pub async fn register_and_start_dmmonitor_service(
    dminit: &DminitConfig,
    monitor_ini_path: &str,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let script = format!("{}/script/root/dm_service_installer.sh", dminit.install_path);
    let register_cmd = format!(
        "bash {} -t dmmonitor -monitor_ini {}",
        shell_quote(&script),
        shell_quote(monitor_ini_path),
    );
    let (stdout, exit_code) = runner
        .exec(&register_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("执行 dm_service_installer.sh 失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "dmmonitor 服务注册失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    start_and_enable_remote_service("DmMonitorService", runner).await
}

async fn start_and_enable_remote_service(name: &str, runner: &dyn CommandRunner) -> Result<()> {
    let (stdout, exit_code) = runner
        .exec(&format!("systemctl start {}", shell_quote(name)))
        .await
        .map_err(|e| anyhow::anyhow!("启动服务 {} 失败: {}", name, e))?;
    anyhow::ensure!(
        exit_code == 0,
        "systemctl start {} 失败 (exit {}): {}",
        name,
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    if let Err(e) = runner.exec(&format!("systemctl enable {}", shell_quote(name))).await {
        tracing::warn!("systemctl enable {} 失败，服务已启动但未设置开机自启: {}", name, e);
    }
    Ok(())
}

/// 通过 disql 查询 V$INSTANCE 验证节点角色和状态。
///
/// 主节点期望 MODE$=PRIMARY, STATUS$=OPEN。
/// 备节点期望 MODE$=STANDBY（STATUS$ 可为 MOUNT 或 OPEN）。
pub async fn verify_node_role(
    node: &NodeConfig,
    dminit: &DminitConfig,
    expected_role: NodeRole,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let cmd = format!(
        "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
        shell_quote(&dminit.install_path),
        shell_quote(&dminit.sysdba_password),
        dminit.port,
    );
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("验证节点角色 disql 执行失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "验证节点角色 disql 失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    let output = String::from_utf8_lossy(&stdout);
    let expected_mode = match expected_role {
        NodeRole::Primary => "PRIMARY",
        NodeRole::Standby => "STANDBY",
        NodeRole::Monitor => anyhow::bail!("monitor 节点不支持角色验证"),
    };
    anyhow::ensure!(
        output.contains(expected_mode),
        "节点 {} ({:?}) 角色验证失败：期望 MODE$={}，实际输出:\n{}",
        node.host, node.role, expected_mode, output
    );
    if expected_role == NodeRole::Primary {
        anyhow::ensure!(
            output.contains("OPEN"),
            "主节点 {} 状态验证失败：期望 STATUS$=OPEN，实际输出:\n{}",
            node.host, output
        );
    }
    if expected_role == NodeRole::Standby && node.read_only {
        anyhow::ensure!(
            output.contains("OPEN"),
            "只读备节点 {} STATUS$ 验证失败：期望 STATUS$=OPEN，实际:\n{}",
            node.host, output
        );
    }
    tracing::info!("[node:{:?}] 角色验证通过 MODE$={}", node.role, expected_mode);
    Ok(())
}

/// 通过 disql 将只读备节点从 MOUNT 状态开启为 open read only（读写分离专用）。
pub async fn configure_read_only_standby(
    node: &NodeConfig,
    dminit: &DminitConfig,
    runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("[node:{:?}] 开启只读备节点 (alter database open read only)", node.role);
    let cmd = format!(
        "echo 'alter database open read only;' | {}/bin/disql SYSDBA/{}@localhost:{}",
        shell_quote(&dminit.install_path),
        shell_quote(&dminit.sysdba_password),
        dminit.port,
    );
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("disql 开启只读模式失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "alter database open read only 失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    Ok(())
}

/// 通过 disql 配置 SQL 日志参数（需在数据库 open 后调用）。
pub async fn configure_sqllog(
    node: &NodeConfig,
    dminit: &DminitConfig,
    sqllog: &SqlLogConfig,
    runner: &dyn CommandRunner,
) -> Result<()> {
    if !sqllog.enabled {
        return Ok(());
    }
    let sql = format!(
        "SP_SET_PARA_VALUE(1,'SVR_LOG','1');\
         SP_SET_SQLLOG_PARA_VALUE('SLOG_ALL','MIN_EXEC_TIME','{}');\
         SP_SET_SQLLOG_PARA_VALUE('SLOG_ALL','FILE_SIZE','{}');\
         SP_SET_SQLLOG_PARA_VALUE('SLOG_ALL','FILE_NUM','{}');",
        sqllog.min_exec_time, sqllog.file_size, sqllog.file_num,
    );
    let cmd = format!(
        "echo \"{}\" | {}/bin/disql SYSDBA/{}@localhost:{}",
        sql,
        shell_quote(&dminit.install_path),
        shell_quote(&dminit.sysdba_password),
        dminit.port,
    );
    for attempt in 1..=6u32 {
        let (stdout, exit_code) = runner
            .exec(&cmd)
            .await
            .map_err(|e| anyhow::anyhow!("disql 执行失败: {}", e))?;
        if exit_code == 0 {
            tracing::info!("[node:{:?}] SQL 日志配置完成", node.role);
            return Ok(());
        }
        let output = String::from_utf8_lossy(&stdout);
        if attempt < 6 {
            tracing::warn!(
                "[node:{:?}] SQL 日志配置失败，数据库可能尚未 open（{}/6）: {}",
                node.role, attempt, output.trim()
            );
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        } else {
            anyhow::bail!("SQL 日志配置失败 (exit {}): {}", exit_code, output);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ssh::MockRunner;
    use crate::config::cluster::{DminitConfig, NodeConfig, NodeRole, SshCredentials};

    fn make_dminit() -> DminitConfig {
        DminitConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            port: 5236,
            page_size: 8,
            charset: 0,
            case_sensitive: true,
            extent_size: 16,
            ..Default::default()
        }
    }

    fn make_primary_node() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Primary,
            host: "192.168.1.10".to_string(),
            instance_name: "DMSVR01".to_string(),
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            read_only: false,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: None,
                password: Some("pass".to_string()),
                port: 22,
            },
        }
    }

    fn make_standby_node() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Standby,
            host: "192.168.1.11".to_string(),
            instance_name: "DMSVR02".to_string(),
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            read_only: false,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: None,
                password: Some("pass".to_string()),
                port: 22,
            },
        }
    }

    #[test]
    fn test_build_dminit_args_format() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let args = build_dminit_args(&node, &dminit);
        assert_eq!(args[0], "'/opt/dmdbms'/bin/dminit", "第一项应为 shell_quote 包裹的 dminit 路径");
        assert!(args.contains(&"PATH='/opt/dmdbms/data'".to_string()), "应含 shell_quote 包裹的 PATH=");
        assert!(args.contains(&"INSTANCE_NAME='DMSVR01'".to_string()), "应含 shell_quote 包裹的 INSTANCE_NAME=");
        assert!(args.contains(&"PORT_NUM=5236".to_string()), "应含 PORT_NUM=（数值无需 shell_quote）");
        assert!(args.contains(&"PAGE_SIZE=8".to_string()), "应含 PAGE_SIZE=");
    }

    #[tokio::test]
    async fn test_distribute_configs_calls_four_sftp_writes() {
        let primary = make_primary_node();
        let standby = make_standby_node();
        let dminit = make_dminit();
        let all_nodes = vec![primary.clone(), standby.clone()];
        let runner = MockRunner::new(vec![]);
        use crate::config::cluster::{ArchiveConfig, MalConfig, WatcherConfig};
        distribute_configs(
            &primary, &dminit, &all_nodes, 453331,
            &ArchiveConfig::default(), &MalConfig::default(), &WatcherConfig::default(),
            &runner,
        )
        .await
        .unwrap();
        let log = runner.sftp_log();
        assert!(log.len() >= 4, "应有 >= 4 次 sftp_write，实际 {}", log.len());
        let paths: Vec<&str> = log.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.iter().any(|p| p.contains("dm.ini")), "应含 dm.ini");
        assert!(paths.iter().any(|p| p.contains("dmmal.ini")), "应含 dmmal.ini");
        assert!(paths.iter().any(|p| p.contains("dmarch.ini")), "应含 dmarch.ini");
        assert!(paths.iter().any(|p| p.contains("dmwatcher.ini")), "应含 dmwatcher.ini");
    }

    #[tokio::test]
    async fn test_configure_database_role_primary_sql() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        configure_database_role(&node, &dminit, NodeRole::Primary, 453331, &runner)
            .await
            .unwrap();
        let log = runner.exec_log();
        let found = log.iter().any(|cmd| {
            cmd.contains("sp_set_oguid(453331)") && cmd.contains("alter database primary")
        });
        assert!(found, "应含 sp_set_oguid(453331) 和 alter database primary: {:?}", log);
    }

    #[tokio::test]
    async fn test_configure_database_role_standby_sql() {
        let node = make_standby_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        configure_database_role(&node, &dminit, NodeRole::Standby, 453331, &runner)
            .await
            .unwrap();
        let log = runner.exec_log();
        let found = log.iter().any(|cmd| {
            cmd.contains("alter database standby") && !cmd.contains("alter database primary")
        });
        assert!(found, "应含 alter database standby 不含 primary: {:?}", log);
    }

    #[tokio::test]
    async fn test_start_dmserver_mount_uses_mount_and_nohup() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        start_dmserver_mount(&node, &dminit, &runner).await.unwrap();
        let log = runner.exec_log();
        let found = log.iter().any(|cmd| {
            cmd.contains("dmserver") && cmd.contains("mount") && cmd.contains("nohup")
        });
        assert!(found, "命令应含 dmserver/mount/nohup: {:?}", log);
    }

    #[tokio::test]
    async fn test_upload_installer_and_install_pushes_xml() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let _ = upload_installer_and_install(&node, &dminit, tmp.path(), &runner).await;
        let sftp_log = runner.sftp_log();
        let exec_log = runner.exec_log();
        let has_bin = sftp_log.iter().any(|(p, _)| p.ends_with(".bin"));
        let has_iso = sftp_log.iter().any(|(p, _)| p.ends_with(".iso"));
        let has_xml = sftp_log.iter().any(|(p, _)| p.contains(".xml"));
        assert!(has_xml, "sftp_log 应含 .xml 路径: {:?}", sftp_log.iter().map(|(p,_)| p).collect::<Vec<_>>());
        assert!(has_bin, "sftp_log 应含 .bin 路径（CR-01）: {:?}", sftp_log.iter().map(|(p,_)| p).collect::<Vec<_>>());
        assert!(!has_iso, "sftp_log 不应含 .iso 路径（CR-01 修复后）");
        let has_chmod = exec_log.iter().any(|cmd| cmd.contains("chmod +x"));
        assert!(has_chmod, "exec_log 应含 chmod +x 调用（CR-01）: {:?}", exec_log);
    }

    #[test]
    fn test_shell_quote_single_quotes_path() {
        assert_eq!(shell_quote("/opt/dmdbms"), "'/opt/dmdbms'");
    }

    #[test]
    fn test_shell_quote_escapes_embedded_single_quote() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_quote_blocks_injection() {
        let result = shell_quote("/tmp; rm -rf /");
        assert!(result.starts_with('\''), "结果应以单引号开头");
        assert!(result.ends_with('\''), "结果应以单引号结尾");
        assert!(result.contains("; rm -rf /"), "注入字符应被包裹在单引号内");
    }

    #[tokio::test]
    async fn test_start_dmserver_mount_quotes_paths() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        start_dmserver_mount(&node, &dminit, &runner).await.unwrap();
        let log = runner.exec_log();
        let found = log.iter().any(|cmd| cmd.contains("'/opt/dmdbms'"));
        assert!(found, "命令应含经 shell_quote 包裹的 install_path: {:?}", log);
    }

    #[tokio::test]
    async fn test_configure_sqllog_disabled_skips_disql() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        configure_sqllog(&node, &dminit, &SqlLogConfig::default(), &runner).await.unwrap();
        assert!(runner.exec_log().is_empty(), "sqllog 禁用时不应执行任何命令");
    }

    #[tokio::test]
    async fn test_configure_sqllog_enabled_sends_sql() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        let cfg = SqlLogConfig { enabled: true, file_size: 128, file_num: 64, min_exec_time: 500 };
        configure_sqllog(&node, &dminit, &cfg, &runner).await.unwrap();
        let log = runner.exec_log();
        assert!(!log.is_empty(), "应执行 disql 命令");
        let cmd = &log[0];
        assert!(cmd.contains("SVR_LOG"), "应含 SVR_LOG");
        assert!(cmd.contains("SLOG_ALL"), "应含 SLOG_ALL");
        assert!(cmd.contains("MIN_EXEC_TIME"), "应含 MIN_EXEC_TIME");
        assert!(cmd.contains("FILE_SIZE"), "应含 FILE_SIZE");
        assert!(cmd.contains("FILE_NUM"), "应含 FILE_NUM");
        assert!(cmd.contains("128"), "应含自定义 file_size 值");
        assert!(cmd.contains("500"), "应含自定义 min_exec_time 值");
    }

    #[tokio::test]
    async fn test_start_dmserver_normal_no_mount_keyword() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        start_dmserver_normal(&node, &dminit, &runner).await.unwrap();
        let log = runner.exec_log();
        let cmd = &log[0];
        assert!(cmd.contains("dmserver") && cmd.contains("nohup"), "应含 dmserver 和 nohup");
        assert!(!cmd.contains(" mount"), "正常启动命令不应含 mount 关键字");
        assert!(cmd.contains("dm.ini"), "应含 dm.ini 路径");
    }

    #[tokio::test]
    async fn test_stop_dmserver_uses_shutdown_immediate() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        stop_dmserver(&node, &dminit, &runner).await.unwrap();
        let log = runner.exec_log();
        let found = log.iter().any(|cmd| cmd.contains("shutdown immediate") && cmd.contains("disql"));
        assert!(found, "应含 shutdown immediate 和 disql: {:?}", log);
    }

    #[tokio::test]
    async fn test_run_dmrman_backup_command_format() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        run_dmrman_backup(&node, &dminit, "/tmp/dm_backup_DMSVR01", &runner).await.unwrap();
        let log = runner.exec_log();
        let found = log.iter().any(|cmd| cmd.contains("dmrman") && cmd.contains("BACKUP") && cmd.contains("BACKUPSET"));
        assert!(found, "应含 dmrman BACKUP BACKUPSET: {:?}", log);
    }

    #[tokio::test]
    async fn test_run_dmrman_restore_three_steps() {
        let node = make_standby_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        run_dmrman_restore(&node, &dminit, "/tmp/dm_backup_DMSVR01", &runner).await.unwrap();
        let log = runner.exec_log();
        assert!(log.iter().any(|c| c.contains("RESTORE")), "应含 RESTORE 步骤: {:?}", log);
        assert!(log.iter().any(|c| c.contains("RECOVER") && c.contains("STANDBY")), "应含 RECOVER FOR STANDBY: {:?}", log);
        assert!(log.iter().any(|c| c.contains("DB_MAGIC")), "应含 UPDATE DB_MAGIC: {:?}", log);
        assert_eq!(log.iter().filter(|c| c.contains("dmrman")).count(), 3, "应有 3 次 dmrman 调用");
    }

    #[tokio::test]
    async fn test_download_backup_files_empty_when_find_returns_nothing() {
        let runner = MockRunner::new(vec![]);
        let files = download_backup_files(&runner, "/tmp/dm_backup_DMSVR01").await.unwrap();
        assert!(files.is_empty(), "find 返回空时应返回空列表");
    }

    #[tokio::test]
    async fn test_upload_backup_files_creates_dir_and_writes() {
        let runner = MockRunner::new(vec![]);
        let files = vec![("meta/backup.meta".to_string(), b"meta_content".to_vec())];
        upload_backup_files(&runner, "/tmp/dm_backup", &files).await.unwrap();
        let exec_log = runner.exec_log();
        assert!(exec_log.iter().any(|c| c.contains("mkdir")), "应含 mkdir 命令: {:?}", exec_log);
        let sftp_log = runner.sftp_log();
        assert!(sftp_log.iter().any(|(p, _)| p.contains("backup.meta")), "应含上传的文件路径: {:?}", sftp_log);
    }

    #[tokio::test]
    async fn test_register_and_start_dmwatcher_service_calls_installer_and_systemctl() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        register_and_start_dmwatcher_service(&node, &dminit, &runner).await.unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|c| c.contains("dm_service_installer.sh") && c.contains("-t dmwatcher")),
            "应调用 dm_service_installer.sh -t dmwatcher: {:?}", log
        );
        assert!(
            log.iter().any(|c| c.contains("dmwatcher.ini")),
            "应传入 dmwatcher.ini 路径: {:?}", log
        );
        assert!(
            log.iter().any(|c| c.contains("systemctl start") && c.contains("DmWatcherService")),
            "应调用 systemctl start DmWatcherService: {:?}", log
        );
    }

    #[tokio::test]
    async fn test_register_and_start_dmmonitor_service_calls_installer_and_systemctl() {
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        register_and_start_dmmonitor_service(&dminit, "/tmp/dmmonitor.ini", &runner)
            .await
            .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|c| c.contains("dm_service_installer.sh") && c.contains("-t dmmonitor")),
            "应调用 dm_service_installer.sh -t dmmonitor: {:?}", log
        );
        assert!(
            log.iter().any(|c| c.contains("systemctl start") && c.contains("DmMonitorService")),
            "应调用 systemctl start DmMonitorService: {:?}", log
        );
        assert!(
            log.iter().any(|c| c.contains("systemctl enable") && c.contains("DmMonitorService")),
            "应调用 systemctl enable DmMonitorService: {:?}", log
        );
    }

    #[tokio::test]
    async fn test_register_and_start_dmmonitor_service_passes_ini_path() {
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        register_and_start_dmmonitor_service(&dminit, "/tmp/dmmonitor.ini", &runner)
            .await
            .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|c| c.contains("-monitor_ini") && c.contains("/tmp/dmmonitor.ini")),
            "应将 monitor_ini 路径传给注册脚本: {:?}", log
        );
    }

    #[tokio::test]
    async fn test_verify_node_role_primary_success() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![(
            "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
            0,
            b"STATUS$   MODE$\nOPEN      PRIMARY\n".to_vec(),
        )]);
        verify_node_role(&node, &dminit, NodeRole::Primary, &runner).await.unwrap();
    }

    #[tokio::test]
    async fn test_verify_node_role_standby_success() {
        let node = make_standby_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![(
            "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
            0,
            b"STATUS$   MODE$\nMOUNT     STANDBY\n".to_vec(),
        )]);
        verify_node_role(&node, &dminit, NodeRole::Standby, &runner).await.unwrap();
    }

    #[tokio::test]
    async fn test_verify_node_role_wrong_mode_returns_err() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![(
            "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
            0,
            b"STATUS$   MODE$\nMOUNT     STANDBY\n".to_vec(),
        )]);
        let err = verify_node_role(&node, &dminit, NodeRole::Primary, &runner).await.unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("PRIMARY"), "错误应含期望角色 PRIMARY: {msg}");
    }

    #[tokio::test]
    async fn test_verify_node_role_primary_not_open_returns_err() {
        let node = make_primary_node();
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![(
            "echo 'SELECT STATUS$,MODE$ FROM V$INSTANCE;'".to_string(),
            0,
            b"STATUS$   MODE$\nMOUNT     PRIMARY\n".to_vec(),
        )]);
        let err = verify_node_role(&node, &dminit, NodeRole::Primary, &runner).await.unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("OPEN"), "错误应含期望状态 OPEN: {msg}");
    }
}
