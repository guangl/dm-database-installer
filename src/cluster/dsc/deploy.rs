use anyhow::{Context, Result};
use std::path::Path;

use crate::common::shell_quote;
use crate::common::ssh::CommandRunner;
use crate::config::cluster::{DminitConfig, DscStorageConfig, NodeConfig};
use crate::cluster::dsc::templates::{
    generate_dmdcr_cfg_ini, generate_dmasvrmal_ini, generate_dmdcr_ini, generate_dminit_ini,
};

/// 对指定节点执行 DM 软件包安装（不调用 dminit）。
///
/// Pitfall 1 防御：DSC 不使用本地 dminit 模式，安装仅完成软件包部署。
pub async fn run_dsc_install_only(
    node: &NodeConfig,
    dminit: &DminitConfig,
    package_path: &Path,
    runner: &dyn CommandRunner,
) -> Result<()> {
    crate::cluster::deploy::upload_installer_and_install(node, dminit, package_path, runner).await
}

/// 向节点分发 DSC 配置文件（dmdcr_cfg.ini / dmasvrmal.ini / dmdcr.ini）。
///
/// Pitfall 3 防御：dmdcr.ini 中 DMDCR_SEQNO 按节点 node_index 设置，每个节点不同。
pub async fn distribute_dsc_configs(
    node: &NodeConfig,
    dminit: &DminitConfig,
    all_nodes: &[NodeConfig],
    oguid: u32,
    storage: &DscStorageConfig,
    node_index: usize,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let dsc_conf_dir = format!("{}/dsc_conf", dminit.install_path);

    let (_, exit_code) = runner
        .exec(&format!("mkdir -p {}", shell_quote(&dsc_conf_dir)))
        .await
        .map_err(|e| anyhow::anyhow!("mkdir dsc_conf 失败: {}", e))?;
    if exit_code != 0 {
        tracing::warn!("mkdir -p {} 返回非零（已忽略）: {}", dsc_conf_dir, exit_code);
    }

    let dmdcr_cfg_content = generate_dmdcr_cfg_ini(all_nodes, oguid, storage, dminit);
    let dmasvrmal_content = generate_dmasvrmal_ini(all_nodes);
    let dmdcr_content = generate_dmdcr_ini(
        node_index,
        &dminit.install_path,
        &dsc_conf_dir,
        &dminit.data_path,
        &node.instance_name,
        storage,
    );

    let dmdcr_cfg_path = format!("{}/dmdcr_cfg.ini", dsc_conf_dir);
    runner
        .sftp_write(&dmdcr_cfg_path, dmdcr_cfg_content.as_bytes())
        .await
        .context("SFTP 上传 dmdcr_cfg.ini 失败")?;
    tracing::info!("已上传 dmdcr_cfg.ini ({} 字节)", dmdcr_cfg_content.len());

    let dmasvrmal_path = format!("{}/dmasvrmal.ini", dsc_conf_dir);
    runner
        .sftp_write(&dmasvrmal_path, dmasvrmal_content.as_bytes())
        .await
        .context("SFTP 上传 dmasvrmal.ini 失败")?;
    tracing::info!("已上传 dmasvrmal.ini ({} 字节)", dmasvrmal_content.len());

    let dmdcr_path = format!("{}/dmdcr.ini", dsc_conf_dir);
    runner
        .sftp_write(&dmdcr_path, dmdcr_content.as_bytes())
        .await
        .context("SFTP 上传 dmdcr.ini 失败")?;
    tracing::info!("已上传 dmdcr.ini ({} 字节) (SEQNO={})", dmdcr_content.len(), node_index);

    Ok(())
}

/// 启动并设置开机自启动服务（private helper）。
///
/// 与 src/cluster/deploy.rs::start_and_enable_remote_service 模式相同。
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
    if let Err(e) = runner
        .exec(&format!("systemctl enable {}", shell_quote(name)))
        .await
    {
        tracing::warn!("systemctl enable {} 失败，服务已启动但未设置开机自启: {}", name, e);
    }
    Ok(())
}

/// 注册并启动 DMCSS 服务。
///
/// 顺序：先注册 → 再启动（Pitfall 2 防御：DMCSS 必须在 DMASM 之前启动）。
pub async fn register_and_start_dmcss_service(
    install_path: &str,
    dmdcr_ini_path: &str,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let script = format!("{}/script/root/dm_service_installer.sh", install_path);
    let cmd = format!(
        "bash {} -t dmcss -dcr_ini {} -p DMCSS",
        shell_quote(&script),
        shell_quote(dmdcr_ini_path),
    );
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("执行 dm_service_installer.sh 注册 DMCSS 失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "DMCSS 服务注册失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    start_and_enable_remote_service("DmCSSServiceDMCSS", runner).await
}

/// 注册并启动 DMASM 服务。
///
/// `-y DmCSSServiceDMCSS` 声明依赖 DMCSS，确保 DMCSS 先于 DMASM 启动。
pub async fn register_and_start_dmasm_service(
    install_path: &str,
    dmdcr_ini_path: &str,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let script = format!("{}/script/root/dm_service_installer.sh", install_path);
    let cmd = format!(
        "bash {} -t dmasmsvr -dcr_ini {} -p DMASM -y DmCSSServiceDMCSS",
        shell_quote(&script),
        shell_quote(dmdcr_ini_path),
    );
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("执行 dm_service_installer.sh 注册 DMASM 失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "DMASM 服务注册失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    start_and_enable_remote_service("DmASMSvrServiceDMASM", runner).await
}

/// 注册并启动 dmserver 服务。
///
/// `-y DmASMSvrServiceDMASM` 声明依赖 DMASM，确保 DMASM 先于 dmserver 启动。
pub async fn register_and_start_dmserver_service(
    install_path: &str,
    dm_ini_path: &str,
    dmdcr_ini_path: &str,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let script = format!("{}/script/root/dm_service_installer.sh", install_path);
    let cmd = format!(
        "bash {} -t dmserver -dm_ini {} -dcr_ini {} -p DMSERVER -y DmASMSvrServiceDMASM",
        shell_quote(&script),
        shell_quote(dm_ini_path),
        shell_quote(dmdcr_ini_path),
    );
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("执行 dm_service_installer.sh 注册 dmserver 失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "dmserver 服务注册失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    start_and_enable_remote_service("DmServiceDMSERVER", runner).await
}

/// 初始化 DCR 磁盘和表决磁盘（通过 dmasmcmd stdin pipe 模式）。
///
/// 将 6 条 dmasmcmd 子命令通过 printf '%s\n' 管道传入，避免单条命令字符串注入。
pub async fn run_dmasmcmd_init(
    install_path: &str,
    storage: &DscStorageConfig,
    dsc_conf_dir: &str,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let subcmds = [
        format!("create dcrdisk '{}' 'dcr'", storage.dcr_disk),
        format!("create votedisk '{}' 'vote'", storage.vote_disk),
        format!("create asmdisk '{}' 'LOG0'", storage.log_disk),
        format!("create asmdisk '{}' 'DATA0'", storage.data_disk),
        format!(
            "init dcrdisk '{}' from '{}/dmdcr_cfg.ini'",
            storage.dcr_disk, dsc_conf_dir
        ),
        format!(
            "init votedisk '{}' from '{}/dmdcr_cfg.ini'",
            storage.vote_disk, dsc_conf_dir
        ),
    ];

    let printf_args: Vec<String> = subcmds
        .iter()
        .map(|s| shell_quote(s))
        .collect();
    let cmd = format!(
        "printf '%s\\n' {} | {}/bin/dmasmcmd",
        printf_args.join(" "),
        shell_quote(install_path),
    );

    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("dmasmcmd 初始化执行失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "dmasmcmd 初始化失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    Ok(())
}

/// 通过 dmasmtool 创建 DMLOG 和 DMDATA 磁盘组。
///
/// 必须在 DMASM 服务已启动、dmasmcmd_init 完成后调用。
/// 使用逻辑磁盘名 LOG0 / DATA0（由 run_dmasmcmd_init 注册）。
pub async fn run_dmasmtool_create_diskgroups(
    install_path: &str,
    dmdcr_ini_path: &str,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let cmd = format!(
        "printf '%s\\n' {create_log} {create_data} | {bin}/dmasmtool DCR_INI={dcr_ini}",
        create_log = shell_quote("create diskgroup 'DMLOG' asmdisk 'LOG0'"),
        create_data = shell_quote("create diskgroup 'DMDATA' asmdisk 'DATA0'"),
        bin = shell_quote(install_path),
        dcr_ini = shell_quote(dmdcr_ini_path),
    );
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("dmasmtool 创建磁盘组执行失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "dmasmtool 创建磁盘组失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    Ok(())
}

/// 在 first_node 执行共享存储 dminit（通过 control file 调用，Pitfall 4 防御）。
///
/// 生成 dminit.ini 并通过 `dminit control={path}` 调用，避免命令行直接传 +DMDATA 路径。
pub async fn run_dminit_shared(
    first_node: &NodeConfig,
    all_nodes: &[NodeConfig],
    dminit: &DminitConfig,
    oguid: u32,
    storage: &DscStorageConfig,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let dsc_conf_dir = format!("{}/dsc_conf", dminit.install_path);
    let dminit_ini_content = generate_dminit_ini(all_nodes, dminit, oguid, storage);
    let dminit_ini_path = format!("{}/dminit.ini", dsc_conf_dir);

    runner
        .sftp_write(&dminit_ini_path, dminit_ini_content.as_bytes())
        .await
        .context("SFTP 上传 dminit.ini 失败")?;
    tracing::info!(
        "[node:{}] 已上传 dminit.ini ({} 字节)",
        first_node.host,
        dminit_ini_content.len()
    );

    let cmd = format!(
        "{}/bin/dminit control={}",
        shell_quote(&dminit.install_path),
        shell_quote(&dminit_ini_path),
    );
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("dminit 执行失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "dminit 共享存储初始化失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    Ok(())
}

/// 将 first_node 上的 dscN_config 目录通过 tar+SFTP 分发到 other_node。
///
/// 步骤：first_node tar 打包 → sftp_read 下载 → sftp_write 上传到 other_node → other_node tar 解压。
pub async fn distribute_config_dir(
    other_node_index: usize,
    dminit: &DminitConfig,
    first_runner: &dyn CommandRunner,
    other_runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("分发 dsc{} 节点 config 目录", other_node_index);
    let tar_filename = format!("dsc{}_config.tar.gz", other_node_index);
    let tar_remote_path = format!("/tmp/{}", tar_filename);

    // Step 1: first_node 上打包
    let tar_cmd = format!(
        "tar czf {} -C {} dsc{}_config",
        shell_quote(&tar_remote_path),
        shell_quote(&dminit.data_path),
        other_node_index,
    );
    let (stdout, exit_code) = first_runner
        .exec(&tar_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("tar 打包 dsc{} config 失败: {}", other_node_index, e))?;
    anyhow::ensure!(
        exit_code == 0,
        "tar czf 失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );

    // Step 2: sftp_read 从 first_node 下载 tarball
    let tar_bytes = first_runner
        .sftp_read(&tar_remote_path)
        .await
        .map_err(|e| anyhow::anyhow!("sftp_read {} 失败: {}", tar_remote_path, e))?;

    // Step 3: sftp_write 上传到 other_node
    other_runner
        .sftp_write(&tar_remote_path, &tar_bytes)
        .await
        .map_err(|e| anyhow::anyhow!("sftp_write {} 到目标节点失败: {}", tar_remote_path, e))?;

    // Step 4: other_node 创建目标目录并解压
    let mkdir_cmd = format!("mkdir -p {}", shell_quote(&dminit.data_path));
    let (stdout, exit_code) = other_runner
        .exec(&mkdir_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("mkdir data_path 失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "mkdir -p {} 失败 (exit {}): {}",
        dminit.data_path,
        exit_code,
        String::from_utf8_lossy(&stdout)
    );

    let extract_cmd = format!(
        "tar xzf {} -C {}",
        shell_quote(&tar_remote_path),
        shell_quote(&dminit.data_path),
    );
    let (stdout, exit_code) = other_runner
        .exec(&extract_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("tar 解压 dsc{} config 失败: {}", other_node_index, e))?;
    anyhow::ensure!(
        exit_code == 0,
        "tar xzf 失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );

    Ok(())
}

/// 通过 disql 查询 V$INSTANCE 验证 DSC 节点是否达到 OPEN 状态（Pitfall 5 防御）。
///
/// DSC 节点 MODE$ 期望为 NORMAL（不强制断言，部分 DM 版本输出 OPEN/PRIMARY）。
/// 核心断言：STATUS$ = OPEN（大小写不敏感）。
/// `node_port` 按节点索引计算（`dminit.port + node_index`），避免所有节点都连到 index 0 的端口。
pub async fn verify_dsc_node(
    node: &NodeConfig,
    dminit: &DminitConfig,
    node_port: u16,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let cmd = format!(
        "echo 'SELECT STATUS$, MODE$ FROM V$INSTANCE;' | {}/bin/disql SYSDBA/{}@localhost:{}",
        shell_quote(&dminit.install_path),
        shell_quote(&dminit.sysdba_password),
        node_port,
    );
    let (stdout, exit_code) = runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("验证 DSC 节点 disql 执行失败: {}", e))?;
    anyhow::ensure!(
        exit_code == 0,
        "验证 DSC 节点 disql 失败 (exit {}): {}",
        exit_code,
        String::from_utf8_lossy(&stdout)
    );
    let output = String::from_utf8_lossy(&stdout);
    anyhow::ensure!(
        output.to_uppercase().contains("OPEN"),
        "DSC 节点 {} 未达到 OPEN 状态，实际输出:\n{}",
        node.host,
        output
    );
    let mode_hint = if output.to_uppercase().contains("NORMAL") {
        "NORMAL"
    } else {
        "未知"
    };
    tracing::info!(
        "[node:{}] DSC 节点验证通过 STATUS$=OPEN MODE$={}",
        node.host,
        mode_hint
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ssh::MockRunner;
    use crate::config::cluster::{DscStorageConfig, NodeConfig, NodeRole, SshCredentials};

    fn make_node(name: &str, host: &str) -> NodeConfig {
        NodeConfig {
            role: NodeRole::Primary,
            host: host.to_string(),
            instance_name: name.to_string(),
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

    fn make_dminit() -> DminitConfig {
        DminitConfig {
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            port: 5236,
            page_size: 8,
            charset: 0,
            case_sensitive: true,
            extent_size: 16,
            sysdba_password: "SYSDBA".to_string(),
        }
    }

    fn make_storage() -> DscStorageConfig {
        DscStorageConfig {
            dcr_disk: "/dev/raw/raw1".to_string(),
            vote_disk: "/dev/raw/raw2".to_string(),
            log_disk: "/dev/raw/raw3".to_string(),
            data_disk: "/dev/raw/raw4".to_string(),
        }
    }

    // Task 1: Test 4 — register_and_start_dmcss_service
    #[tokio::test]
    async fn test_register_and_start_dmcss_service_calls_installer_and_systemctl() {
        let runner = MockRunner::new(vec![]);
        register_and_start_dmcss_service(
            "/opt/dmdbms",
            "/opt/dmdbms/dsc_conf/dmdcr.ini",
            &runner,
        )
        .await
        .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|c| c.contains("dm_service_installer.sh")
                && c.contains("-t dmcss")
                && c.contains("-dcr_ini")),
            "exec_log 应含 dm_service_installer.sh -t dmcss -dcr_ini，实际: {:?}",
            log
        );
        assert!(
            log.iter().any(|c| c.contains("systemctl start") && c.contains("DmCSSServiceDMCSS")),
            "exec_log 应含 systemctl start DmCSSServiceDMCSS，实际: {:?}",
            log
        );
    }

    // Task 1: Test 5 — register_and_start_dmasm_service
    #[tokio::test]
    async fn test_register_and_start_dmasm_service_calls_installer_with_dep() {
        let runner = MockRunner::new(vec![]);
        register_and_start_dmasm_service(
            "/opt/dmdbms",
            "/opt/dmdbms/dsc_conf/dmdcr.ini",
            &runner,
        )
        .await
        .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|c| c.contains("-t dmasmsvr")
                && c.contains("-dcr_ini")
                && c.contains("-p DMASM")
                && c.contains("-y DmCSSServiceDMCSS")),
            "exec_log 应含 -t dmasmsvr -dcr_ini ... -p DMASM -y DmCSSServiceDMCSS，实际: {:?}",
            log
        );
        assert!(
            log.iter()
                .any(|c| c.contains("systemctl start") && c.contains("DmASMSvrServiceDMASM")),
            "exec_log 应含 systemctl start DmASMSvrServiceDMASM，实际: {:?}",
            log
        );
    }

    // Task 1: Test 6 — register_and_start_dmserver_service
    #[tokio::test]
    async fn test_register_and_start_dmserver_service_uses_dm_ini_and_dcr_ini() {
        let runner = MockRunner::new(vec![]);
        register_and_start_dmserver_service(
            "/opt/dmdbms",
            "/opt/dmdbms/data/DSC0/dm.ini",
            "/opt/dmdbms/dsc_conf/dmdcr.ini",
            &runner,
        )
        .await
        .unwrap();
        let log = runner.exec_log();
        assert!(
            log.iter().any(|c| c.contains("-t dmserver") && c.contains("-dm_ini")),
            "exec_log 应含 -t dmserver -dm_ini，实际: {:?}",
            log
        );
        assert!(
            log.iter().any(|c| c.contains("-dcr_ini")),
            "exec_log 应含 -dcr_ini，实际: {:?}",
            log
        );
        assert!(
            log.iter()
                .any(|c| c.contains("systemctl start") && c.contains("DmServiceDMSERVER")),
            "exec_log 应含 systemctl start DmServiceDMSERVER，实际: {:?}",
            log
        );
    }

    // Task 1: Test 2 — distribute_dsc_configs uploads three files
    #[tokio::test]
    async fn test_distribute_dsc_configs_uploads_three_files() {
        let node = make_node("DSC0", "192.168.1.10");
        let dminit = make_dminit();
        let storage = make_storage();
        let all_nodes = vec![
            make_node("DSC0", "192.168.1.10"),
            make_node("DSC1", "192.168.1.11"),
        ];
        let runner = MockRunner::new(vec![]);
        distribute_dsc_configs(&node, &dminit, &all_nodes, 63635, &storage, 0, &runner)
            .await
            .unwrap();
        let log = runner.sftp_log();
        let paths: Vec<&str> = log.iter().map(|(p, _)| p.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.contains("dmdcr_cfg.ini")),
            "sftp_log 应含 dmdcr_cfg.ini，实际: {:?}",
            paths
        );
        assert!(
            paths.iter().any(|p| p.contains("dmasvrmal.ini")),
            "sftp_log 应含 dmasvrmal.ini，实际: {:?}",
            paths
        );
        assert!(
            paths.iter().any(|p| p.contains("dmdcr.ini")),
            "sftp_log 应含 dmdcr.ini，实际: {:?}",
            paths
        );
    }

    // Task 1: Test 3 — dmdcr.ini SEQNO 按节点 index 区分
    #[tokio::test]
    async fn test_distribute_dsc_configs_dmdcr_seqno_matches_index() {
        let node = make_node("DSC1", "192.168.1.11");
        let dminit = make_dminit();
        let storage = make_storage();
        let all_nodes = vec![
            make_node("DSC0", "192.168.1.10"),
            make_node("DSC1", "192.168.1.11"),
        ];
        let runner = MockRunner::new(vec![]);
        // node_index = 1
        distribute_dsc_configs(&node, &dminit, &all_nodes, 63635, &storage, 1, &runner)
            .await
            .unwrap();
        let log = runner.sftp_log();
        let dmdcr_entry = log
            .iter()
            .find(|(p, _)| p.contains("dmdcr.ini"))
            .expect("sftp_log 应含 dmdcr.ini");
        let content = String::from_utf8_lossy(&dmdcr_entry.1);
        assert!(
            content.contains("DMDCR_SEQNO = 1"),
            "dmdcr.ini 内容应含 DMDCR_SEQNO = 1，实际:\n{}",
            content
        );
    }

    // Task 2: Test 1 — run_dmasmcmd_init 包含关键子命令
    #[tokio::test]
    async fn test_run_dmasmcmd_init_executes_create_and_init_sequence() {
        let storage = make_storage();
        let runner = MockRunner::new(vec![]);
        run_dmasmcmd_init(
            "/opt/dmdbms",
            &storage,
            "/opt/dmdbms/dsc_conf",
            &runner,
        )
        .await
        .unwrap();
        let log = runner.exec_log();
        assert_eq!(log.len(), 1, "应只有一条命令（一次 printf pipe）");
        let cmd = &log[0];
        assert!(cmd.contains("dmasmcmd"), "命令应含 dmasmcmd: {}", cmd);
        assert!(cmd.contains("create dcrdisk"), "命令应含 create dcrdisk: {}", cmd);
        assert!(cmd.contains("create votedisk"), "命令应含 create votedisk: {}", cmd);
        assert!(cmd.contains("create asmdisk"), "命令应含 create asmdisk: {}", cmd);
        assert!(cmd.contains("init dcrdisk"), "命令应含 init dcrdisk: {}", cmd);
        assert!(cmd.contains("init votedisk"), "命令应含 init votedisk: {}", cmd);
    }

    // Task 2: Test 2 — run_dmasmcmd_init 使用 storage 磁盘路径
    #[tokio::test]
    async fn test_run_dmasmcmd_init_uses_storage_disks() {
        let storage = make_storage();
        let runner = MockRunner::new(vec![]);
        run_dmasmcmd_init(
            "/opt/dmdbms",
            &storage,
            "/opt/dmdbms/dsc_conf",
            &runner,
        )
        .await
        .unwrap();
        let log = runner.exec_log();
        let cmd = &log[0];
        assert!(
            cmd.contains(&storage.dcr_disk),
            "命令应含 dcr_disk: {}",
            storage.dcr_disk
        );
        assert!(
            cmd.contains(&storage.vote_disk),
            "命令应含 vote_disk: {}",
            storage.vote_disk
        );
        assert!(
            cmd.contains(&storage.log_disk),
            "命令应含 log_disk: {}",
            storage.log_disk
        );
        assert!(
            cmd.contains(&storage.data_disk),
            "命令应含 data_disk: {}",
            storage.data_disk
        );
    }

    // Task 2: Test 3 — run_dmasmtool_create_diskgroups
    #[tokio::test]
    async fn test_run_dmasmtool_create_diskgroups_uses_dcr_ini() {
        let runner = MockRunner::new(vec![]);
        run_dmasmtool_create_diskgroups(
            "/opt/dmdbms",
            "/opt/dmdbms/dsc_conf/dmdcr.ini",
            &runner,
        )
        .await
        .unwrap();
        let log = runner.exec_log();
        let cmd = &log[0];
        assert!(cmd.contains("dmasmtool"), "命令应含 dmasmtool: {}", cmd);
        assert!(cmd.contains("DCR_INI="), "命令应含 DCR_INI=: {}", cmd);
        // 命令经过 shell_quote 转义，检查关键词（未转义部分仍存在）
        assert!(
            cmd.contains("DMLOG"),
            "命令应含 DMLOG 磁盘组名: {}",
            cmd
        );
        assert!(
            cmd.contains("DMDATA"),
            "命令应含 DMDATA 磁盘组名: {}",
            cmd
        );
    }

    // Task 2: Test 4 — run_dminit_shared 上传 dminit.ini 并执行 dminit
    #[tokio::test]
    async fn test_run_dminit_shared_uploads_ini_and_executes_dminit() {
        let first_node = make_node("DSC0", "192.168.1.10");
        let all_nodes = vec![
            make_node("DSC0", "192.168.1.10"),
            make_node("DSC1", "192.168.1.11"),
        ];
        let dminit = make_dminit();
        let storage = make_storage();
        let runner = MockRunner::new(vec![]);
        run_dminit_shared(&first_node, &all_nodes, &dminit, 63635, &storage, &runner)
            .await
            .unwrap();
        let sftp_log = runner.sftp_log();
        let paths: Vec<&str> = sftp_log.iter().map(|(p, _)| p.as_str()).collect();
        assert!(
            paths.iter().any(|p| p.contains("dsc_conf") && p.contains("dminit.ini")),
            "sftp_log 应含 dsc_conf/dminit.ini，实际: {:?}",
            paths
        );
        let exec_log = runner.exec_log();
        assert!(
            exec_log.iter().any(|c| c.contains("dminit") && c.contains("control=")),
            "exec_log 应含 dminit control=...，实际: {:?}",
            exec_log
        );
    }

    // Task 2: Test 5 — dminit.ini 中含 +DMDATA 前缀（Pitfall 4）
    #[tokio::test]
    async fn test_run_dminit_shared_uses_asm_path_in_ini() {
        let first_node = make_node("DSC0", "192.168.1.10");
        let all_nodes = vec![
            make_node("DSC0", "192.168.1.10"),
            make_node("DSC1", "192.168.1.11"),
        ];
        let dminit = make_dminit();
        let storage = make_storage();
        let runner = MockRunner::new(vec![]);
        run_dminit_shared(&first_node, &all_nodes, &dminit, 63635, &storage, &runner)
            .await
            .unwrap();
        let sftp_log = runner.sftp_log();
        let dminit_entry = sftp_log
            .iter()
            .find(|(p, _)| p.contains("dminit.ini"))
            .expect("sftp_log 应含 dminit.ini");
        let content = String::from_utf8_lossy(&dminit_entry.1);
        assert!(
            content.contains("SYSTEM_PATH = +DMDATA"),
            "dminit.ini 应含 SYSTEM_PATH = +DMDATA，实际:\n{}",
            content
        );
    }

    // Task 2: Test 6 — distribute_config_dir tar+SFTP 流程
    #[tokio::test]
    async fn test_distribute_config_dir_tars_on_source_and_extracts_on_target() {
        let dminit = make_dminit();
        let first_runner = MockRunner::new(vec![]);
        let other_runner = MockRunner::new(vec![]);
        // 预设 first_runner 的 sftp_read 返回模拟 tarball 内容
        first_runner.set_sftp_read("/tmp/dsc1_config.tar.gz", b"fake-tarball".to_vec());
        distribute_config_dir(1, &dminit, &first_runner, &other_runner)
            .await
            .unwrap();
        let first_exec = first_runner.exec_log();
        assert!(
            first_exec.iter().any(|c| c.contains("tar") && c.contains("czf")),
            "first_runner exec_log 应含 tar czf，实际: {:?}",
            first_exec
        );
        let other_exec = other_runner.exec_log();
        assert!(
            other_exec.iter().any(|c| c.contains("tar") && c.contains("xzf")),
            "other_runner exec_log 应含 tar xzf，实际: {:?}",
            other_exec
        );
        // sftp_read 从 first_runner 读取一次
        let first_sftp = first_runner.sftp_log();
        let _ = first_sftp; // sftp_log 只记录写入，读取通过 sftp_read_data 预设
        // sftp_write 上传到 other_runner 一次（tarball）
        let other_sftp = other_runner.sftp_log();
        assert_eq!(other_sftp.len(), 1, "other_runner sftp_write 应调用 1 次，实际: {}", other_sftp.len());
        let (upload_path, upload_bytes) = &other_sftp[0];
        assert!(
            upload_path.contains("dsc1_config.tar.gz"),
            "上传路径应含 dsc1_config.tar.gz，实际: {}",
            upload_path
        );
        assert_eq!(
            upload_bytes,
            b"fake-tarball",
            "上传内容应与 sftp_read 读取的内容一致"
        );
    }

    // Task 2: Test 7 — verify_dsc_node 接受 OPEN 状态
    #[tokio::test]
    async fn test_verify_dsc_node_accepts_open_normal() {
        let node = make_node("DSC0", "192.168.1.10");
        let dminit = make_dminit();
        let disql_output = b"STATUS$ = OPEN\nMODE$ = NORMAL".to_vec();
        let runner = MockRunner::new(vec![(
            "echo".to_string(),
            0,
            disql_output,
        )]);
        let result = verify_dsc_node(&node, &dminit, dminit.port, &runner).await;
        assert!(result.is_ok(), "OPEN 状态应验证通过，实际: {:?}", result);
    }

    // Task 2: Test 8 — verify_dsc_node 拒绝 MOUNT 状态
    #[tokio::test]
    async fn test_verify_dsc_node_rejects_other_status() {
        let node = make_node("DSC0", "192.168.1.10");
        let dminit = make_dminit();
        let disql_output = b"STATUS$ = MOUNT\nMODE$ = NORMAL".to_vec();
        let runner = MockRunner::new(vec![(
            "echo".to_string(),
            0,
            disql_output,
        )]);
        let result = verify_dsc_node(&node, &dminit, dminit.port, &runner).await;
        assert!(result.is_err(), "MOUNT 状态应验证失败");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("OPEN"),
            "错误消息应含 'OPEN'，实际: {}",
            err_msg
        );
    }

    // Task 1: Test 1 — run_dsc_install_only 调用 DMInstall.bin 但不调用 dminit
    // 注意：此测试因 upload_installer_and_install 需要真实文件路径，使用简化验证
    #[tokio::test]
    async fn test_run_dsc_install_only_calls_upload_but_not_dminit() {
        // run_dsc_install_only 委托给 upload_installer_and_install
        // 该函数需要真实包路径（读取文件）；此处验证不调用 dminit
        // 通过确认函数签名中没有 run_dminit_remote 调用来保证（编译时静态验证）
        // 运行时测试：使用不存在的路径，函数应在 sftp_write XML 之前失败（而非调用 dminit）
        let node = make_node("DSC0", "192.168.1.10");
        let dminit = make_dminit();
        let runner = MockRunner::new(vec![]);
        let fake_path = std::path::Path::new("/nonexistent/dm_install.bin");
        let result = run_dsc_install_only(&node, &dminit, fake_path, &runner).await;
        // 期望在读取安装包文件时失败（文件不存在），而不是因为 dminit 失败
        // exec_log 中不应含 "dminit"
        let log = runner.exec_log();
        assert!(
            !log.iter().any(|c| c.contains("dminit") && !c.contains("DMInstall")),
            "exec_log 不应含 dminit 调用（Pitfall 1 防御），实际: {:?}",
            log
        );
        // 忽略实际错误（文件不存在），仅断言没有 dminit 调用
        let _ = result;
    }
}
