pub mod templates;
pub mod deploy;

use anyhow::Result;
use futures::future::try_join_all;
use std::sync::Arc;

use crate::cluster::{health, phases};
use crate::common::ssh;
use crate::config::cluster::{ClusterSpecificConfig, DminitConfig, DscStorageConfig, NodeConfig, NodeRole};
use crate::config::{CommonConfig, InstallerSource};

/// DSC 共享存储集群部署入口：建立 SSH 会话后委托 run_with_runners。
pub async fn run(common: CommonConfig, specific: ClusterSpecificConfig) -> Result<()> {
    tracing::info!("[cluster][1/10] 建立 SSH 会话");
    let mut runners: phases::Runners = Vec::new();
    for node in &specific.nodes {
        let session = ssh::SshSession::connect(&node.host, node.ssh.port, &node.ssh)
            .await
            .map_err(|e| anyhow::anyhow!("连接节点 {}:{} 失败: {}", node.host, node.ssh.port, e))?;
        runners.push((node.clone(), Arc::new(session)));
    }
    run_with_runners(
        common,
        specific,
        runners,
        |host, port, secs| {
            Box::pin(async move { health::wait_tcp_ready(&host, port, secs).await })
        },
    )
    .await
}

/// DSC 集群部署核心：接受 runners + health_check_fn，实现 8 个 checkpoint gate。
///
/// - Gate 1: preflight（SSH 预检查）
/// - Gate 2: install（安装软件包，不含 dminit）
/// - Gate 3: dsc_config_distributed（分发 dmdcr_cfg.ini/dmasvrmal.ini/dmdcr.ini）
/// - Gate 4: css_asm_started（所有节点启动 DMCSS + DMASM）
/// - Gate 5: asm_diskgroup_created（first_node 上 dmasmcmd 初始化 + dmasmtool 创建磁盘组）
/// - Gate 6: dminit_shared_done（first_node 上 dminit control= 共享初始化）
/// - Gate 7: config_dir_distributed（first_node → other_nodes 分发 config 目录）
/// - Gate 8: dmserver_started（所有节点注册启动 dmserver + 验证 V$INSTANCE）
pub async fn run_with_runners<F>(
    common: CommonConfig,
    specific: ClusterSpecificConfig,
    runners: phases::Runners,
    health_check_fn: F,
) -> Result<()>
where
    F: Fn(String, u16, u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
        + Send
        + Sync,
{
    let dminit = specific.dminit.clone();
    let oguid = specific.oguid;
    let storage = specific
        .dsc_storage
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("缺少 dsc_storage 配置，请在 dsc.toml 中添加 [dsc_storage] 段"))?
        .clone();

    let mut cp = crate::cluster::checkpoint::ClusterCheckpoint::load()?.unwrap_or_default();

    // Gate 1: preflight
    if !cp.preflight_done {
        tracing::info!("[cluster][2/10] SSH 预检查");
        phases::run_preflight(&runners, &dminit).await?;
        cp.preflight_done = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过预检查（checkpoint）");
    }

    // Gate 2: 安装（不含 dminit）
    if !cp.install_done {
        tracing::info!("[cluster][3/10] 安装软件包（所有节点并行，不含 dminit）");
        run_dsc_install_all_nodes(&common, &runners, &dminit).await?;
        cp.install_done = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过安装（checkpoint）");
    }

    // Gate 3: 分发 DSC 配置
    if !cp.dsc_config_distributed {
        tracing::info!("[cluster][4/10] 分发 DSC 配置文件（并行所有节点）");
        run_distribute_dsc_configs_all_nodes(&runners, &dminit, oguid, &storage).await?;
        cp.dsc_config_distributed = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过 DSC 配置分发（checkpoint）");
    }

    // Gate 4: DMCSS + DMASM 启动（所有节点）
    if !cp.css_asm_started {
        tracing::info!("[cluster][5/10] 启动 DMCSS + DMASM（所有节点）");
        run_start_css_asm_all_nodes(&runners, &dminit, &health_check_fn).await?;
        cp.css_asm_started = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过 DMCSS+DMASM 启动（checkpoint）");
    }

    // Gate 5: ASM 磁盘组初始化（仅 first_node）
    if !cp.asm_diskgroup_created {
        tracing::info!("[cluster][6/10] ASM 磁盘组初始化（first_node）");
        run_asm_init_first_node(&runners, &dminit, &storage).await?;
        cp.asm_diskgroup_created = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过 ASM 磁盘组初始化（checkpoint）");
    }

    // Gate 6: 共享 dminit（仅 first_node）
    if !cp.dminit_shared_done {
        tracing::info!("[cluster][7/10] 共享存储 dminit（first_node）");
        run_dminit_shared_first_node(&runners, &specific.nodes, &dminit, oguid, &storage).await?;
        cp.dminit_shared_done = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过共享 dminit（checkpoint）");
    }

    // Gate 7: config 目录分发（first_node → other_nodes）
    if !cp.config_dir_distributed {
        tracing::info!("[cluster][8/10] 分发 config 目录（first_node → other_nodes）");
        run_distribute_config_dirs(&runners, &dminit).await?;
        cp.config_dir_distributed = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过 config 目录分发（checkpoint）");
    }

    // Gate 8: dmserver 注册启动并验证
    if !cp.dmserver_started {
        tracing::info!("[cluster][9/10] 启动并验证 dmserver（先 first_node 后 others）");
        run_start_and_verify_dmserver_all_nodes(&runners, &dminit, &health_check_fn).await?;
        cp.dmserver_started = true;
        cp.save()?;
    } else {
        tracing::info!("[续] 跳过 dmserver 启动（checkpoint）");
    }

    crate::cluster::checkpoint::ClusterCheckpoint::remove()?;
    tracing::info!("[cluster][10/10] DSC 集群部署完成");
    Ok(())
}

// ─── 私有 helper 函数 ─────────────────────────────────────────────────────────

/// 返回 runners 中第一个 role == Primary 的下标，否则返回错误。
async fn first_node_index(runners: &phases::Runners) -> Result<usize> {
    runners
        .iter()
        .position(|(n, _)| n.role == NodeRole::Primary)
        .ok_or_else(|| anyhow::anyhow!("DSC 集群缺少 primary 节点（first_node）"))
}

/// 在所有节点并行安装 DM 软件包（不调用 dminit，Pitfall 1 防御）。
async fn run_dsc_install_all_nodes(
    common: &CommonConfig,
    runners: &phases::Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    let handle = match &common.installer {
        InstallerSource::Auto => {
            tracing::info!("自动检测本地平台并下载安装包");
            crate::common::download::fetch_dm_installer().await?
        }
        InstallerSource::LocalFile(path) => crate::common::download::PackageHandle::from_path(path.clone()),
        InstallerSource::Url(url) => {
            tracing::info!("下载安装包: {}", url);
            crate::common::download::fetch_from_url(url).await?
        }
    };
    let pkg_path = handle.path.clone();

    let futs: Vec<_> = runners
        .iter()
        .map(|(node, runner)| {
            let node = node.clone();
            let runner = Arc::clone(runner);
            let pkg = pkg_path.clone();
            let dminit = dminit.clone();
            async move {
                deploy::run_dsc_install_only(&node, &dminit, &pkg, runner.as_ref()).await
            }
        })
        .collect();
    try_join_all(futs).await?;
    Ok(())
}

/// 在所有节点并行分发 DSC 配置文件（dmdcr_cfg/dmasvrmal/dmdcr）。
async fn run_distribute_dsc_configs_all_nodes(
    runners: &phases::Runners,
    dminit: &DminitConfig,
    oguid: u32,
    storage: &DscStorageConfig,
) -> Result<()> {
    let all_nodes: Vec<NodeConfig> = runners.iter().map(|(n, _)| n.clone()).collect();
    let futs: Vec<_> = runners
        .iter()
        .enumerate()
        .map(|(node_index, (node, runner))| {
            let node = node.clone();
            let runner = Arc::clone(runner);
            let all_nodes = all_nodes.clone();
            let dminit = dminit.clone();
            let storage = storage.clone();
            async move {
                deploy::distribute_dsc_configs(
                    &node, &dminit, &all_nodes, oguid, &storage, node_index, runner.as_ref(),
                )
                .await
            }
        })
        .collect();
    try_join_all(futs).await?;
    Ok(())
}

/// 在所有节点并行启动 DMCSS，再并行启动 DMASM，最后等待 DMASM 端口就绪。
///
/// 严格顺序：DMCSS 全部完成 → DMASM start + 端口就绪（Pitfall 2 防御）。
async fn run_start_css_asm_all_nodes<F>(
    runners: &phases::Runners,
    dminit: &DminitConfig,
    health_check_fn: &F,
) -> Result<()>
where
    F: Fn(String, u16, u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
        + Send
        + Sync,
{
    let dmdcr_ini_path = format!("{}/dsc_conf/dmdcr.ini", dminit.install_path);

    // 1) 所有节点并行注册并启动 DMCSS
    let css_futs: Vec<_> = runners
        .iter()
        .map(|(node, runner)| {
            let runner = Arc::clone(runner);
            let install_path = dminit.install_path.clone();
            let dmdcr_ini = dmdcr_ini_path.clone();
            let node = node.clone();
            async move {
                tracing::info!("[node:{}] 注册并启动 DMCSS", node.host);
                deploy::register_and_start_dmcss_service(&install_path, &dmdcr_ini, runner.as_ref()).await
            }
        })
        .collect();
    try_join_all(css_futs).await?;

    // 2) 所有节点并行注册并启动 DMASM
    let asm_futs: Vec<_> = runners
        .iter()
        .map(|(node, runner)| {
            let runner = Arc::clone(runner);
            let install_path = dminit.install_path.clone();
            let dmdcr_ini = dmdcr_ini_path.clone();
            let node = node.clone();
            async move {
                tracing::info!("[node:{}] 注册并启动 DMASM", node.host);
                deploy::register_and_start_dmasm_service(&install_path, &dmdcr_ini, runner.as_ref()).await
            }
        })
        .collect();
    try_join_all(asm_futs).await?;

    // 3) 等待所有节点 DMASM 端口就绪（端口按节点 index 递增：9349 + node_idx * 2，与 dmdcr_cfg.ini 保持一致）
    for (node_idx, (node, _)) in runners.iter().enumerate() {
        let asm_port = 9349u16 + (node_idx as u16) * 2;
        tracing::info!("[node:{}] 等待 DMASM 端口 {} 就绪...", node.host, asm_port);
        health_check_fn(node.host.clone(), asm_port, 60).await?;
    }
    Ok(())
}

/// 在 first_node 上初始化 ASM 磁盘和磁盘组（dmasmcmd → dmasmtool）。
async fn run_asm_init_first_node(
    runners: &phases::Runners,
    dminit: &DminitConfig,
    storage: &DscStorageConfig,
) -> Result<()> {
    let first_idx = first_node_index(runners).await?;
    let (first_node, first_runner) = &runners[first_idx];
    tracing::info!("[node:{}] dmasmcmd 初始化磁盘", first_node.host);
    let dsc_conf_dir = format!("{}/dsc_conf", dminit.install_path);
    deploy::run_dmasmcmd_init(&dminit.install_path, storage, &dsc_conf_dir, first_runner.as_ref()).await?;
    let dmdcr_ini_path = format!("{}/dsc_conf/dmdcr.ini", dminit.install_path);
    tracing::info!("[node:{}] dmasmtool 创建磁盘组", first_node.host);
    deploy::run_dmasmtool_create_diskgroups(&dminit.install_path, &dmdcr_ini_path, first_runner.as_ref()).await
}

/// 在 first_node 上执行共享 dminit。
async fn run_dminit_shared_first_node(
    runners: &phases::Runners,
    all_nodes: &[NodeConfig],
    dminit: &DminitConfig,
    oguid: u32,
    storage: &DscStorageConfig,
) -> Result<()> {
    let first_idx = first_node_index(runners).await?;
    let (first_node, first_runner) = &runners[first_idx];
    tracing::info!("[node:{}] 共享存储 dminit", first_node.host);
    deploy::run_dminit_shared(first_node, all_nodes, dminit, oguid, storage, first_runner.as_ref()).await
}

/// 将 first_node 生成的 config 目录分发到其余各节点。
async fn run_distribute_config_dirs(
    runners: &phases::Runners,
    dminit: &DminitConfig,
) -> Result<()> {
    let first_idx = first_node_index(runners).await?;
    let (_, first_runner) = &runners[first_idx];

    for (other_idx, (other_node, other_runner)) in runners.iter().enumerate() {
        if other_idx == first_idx {
            continue;
        }
        tracing::info!("[node:{}] 接收 config 目录 (来自 first_node)", other_node.host);
        deploy::distribute_config_dir(
            first_idx,
            other_idx,
            dminit,
            first_runner.as_ref(),
            other_runner.as_ref(),
        )
        .await?;
    }
    Ok(())
}

/// 在 first_node 先启动 dmserver，再在其余节点依次启动，最后并行验证所有节点。
async fn run_start_and_verify_dmserver_all_nodes<F>(
    runners: &phases::Runners,
    dminit: &DminitConfig,
    health_check_fn: &F,
) -> Result<()>
where
    F: Fn(String, u16, u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>
        + Send
        + Sync,
{
    let first_idx = first_node_index(runners).await?;
    let dmdcr_ini_path = format!("{}/dsc_conf/dmdcr.ini", dminit.install_path);

    // 先在 first_node 启动 dmserver
    let (first_node, first_runner) = &runners[first_idx];
    let dm_ini_path = format!(
        "{}/{}/dm.ini",
        dminit.data_path, first_node.instance_name
    );
    tracing::info!("[node:{}] 注册并启动 dmserver（first_node）", first_node.host);
    deploy::register_and_start_dmserver_service(
        &dminit.install_path,
        &dm_ini_path,
        &dmdcr_ini_path,
        first_runner.as_ref(),
    )
    .await?;
    // first_node 的实际端口按其在数组中的下标（first_idx）计算，与 dminit.ini PORT_NUM 保持一致
    let first_node_port = dminit.port.saturating_add(first_idx as u16);
    health_check_fn(first_node.host.clone(), first_node_port, 60).await?;

    // 再依次启动其他节点 dmserver
    for (node_idx, (node, runner)) in runners.iter().enumerate() {
        if node_idx == first_idx {
            continue;
        }
        let port = dminit.port.saturating_add(node_idx as u16);
        let dm_ini = format!("{}/{}/dm.ini", dminit.data_path, node.instance_name);
        tracing::info!("[node:{}] 注册并启动 dmserver", node.host);
        deploy::register_and_start_dmserver_service(
            &dminit.install_path,
            &dm_ini,
            &dmdcr_ini_path,
            runner.as_ref(),
        )
        .await?;
        health_check_fn(node.host.clone(), port, 60).await?;
    }

    // 最后并行验证所有节点（每个节点按其数组下标计算实际端口）
    let verify_futs: Vec<_> = runners
        .iter()
        .enumerate()
        .map(|(node_idx, (node, runner))| {
            let node = node.clone();
            let runner = Arc::clone(runner);
            let dminit = dminit.clone();
            let node_port = dminit.port.saturating_add(node_idx as u16);
            async move { deploy::verify_dsc_node(&node, &dminit, node_port, runner.as_ref()).await }
        })
        .collect();
    try_join_all(verify_futs).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    use crate::cluster::phases;
    use crate::common::ssh::MockRunner;
    use crate::config::cluster::{
        ClusterSpecificConfig, DminitConfig, DmIniConfig, DscStorageConfig, MalConfig,
        NodeConfig, NodeRole, SqlLogConfig, SshCredentials, WatcherConfig,
    };
    use crate::config::{ArchiveConfig, CommonConfig, InstallerSource, InstallType};

    /// 用于串行化需要 set_current_dir 的测试，避免并发竞争。
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    fn make_node(role: NodeRole, host: &str, name: &str) -> NodeConfig {
        NodeConfig {
            role,
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

    fn make_specific(nodes: Vec<NodeConfig>) -> ClusterSpecificConfig {
        ClusterSpecificConfig {
            oguid: 63635,
            nodes,
            dsc_storage: Some(make_storage()),
            shared_storage: None,
            dminit: make_dminit(),
            dm_ini: DmIniConfig::default(),
            archive: ArchiveConfig::default(),
            mal: MalConfig::default(),
            watcher: WatcherConfig::default(),
            sqllog: SqlLogConfig::default(),
        }
    }

    fn make_common_with_fake_installer(dir: &TempDir) -> CommonConfig {
        // 在临时目录中创建一个假的安装包文件
        let fake_bin = dir.path().join("fake_installer.bin");
        std::fs::write(&fake_bin, b"fake installer binary").unwrap();
        CommonConfig {
            install_type: InstallType::Dsc,
            installer: InstallerSource::LocalFile(fake_bin),
        }
    }

    /// 构造含 preflight 所需响应的 MockRunner。
    ///
    /// preflight 依次调用：sudo -n true、ss -tlnp | grep ':5236'、df -B1 /opt
    /// df 必须返回合法的第 2 行第 4 列（Available >= 5GB）。
    fn make_runner_with_preflight() -> MockRunner {
        // df 输出：第 2 行第 4 列 = 10737418240（10 GB）
        let df_output = b"Filesystem     1B-blocks       Used  Available Use% Mounted on\n\
/dev/sda1    107374182400 10737418240 10737418240  50% /opt\n";
        MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("ss -tlnp | grep ':5236'".to_string(), 0, vec![]),
            ("df -B1 /opt".to_string(), 0, df_output.to_vec()),
        ])
    }

    // Test 1: 已完成的步骤被跳过（6 个 DSC gate 全部 true）
    #[tokio::test]
    async fn test_run_with_runners_skips_completed_steps_from_checkpoint() {
        let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        // 写入 checkpoint，标记全部 gate 为 true
        let cp = crate::cluster::checkpoint::ClusterCheckpoint {
            preflight_done: true,
            install_done: true,
            primary_init_done: false,
            backup_done: false,
            standby_restore_done: false,
            dsc_config_distributed: true,
            css_asm_started: true,
            asm_diskgroup_created: true,
            dminit_shared_done: true,
            config_dir_distributed: true,
            dmserver_started: true,
        };
        cp.save_to(dir.path()).unwrap();

        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let primary = make_node(NodeRole::Primary, "192.168.1.10", "DSC0");
        let standby = make_node(NodeRole::Standby, "192.168.1.11", "DSC1");
        let runner0 = Arc::new(MockRunner::new(vec![]));
        let runner1 = Arc::new(MockRunner::new(vec![]));
        let runners: phases::Runners = vec![
            (primary, Arc::clone(&runner0) as Arc<dyn crate::common::ssh::CommandRunner>),
            (standby, Arc::clone(&runner1) as Arc<dyn crate::common::ssh::CommandRunner>),
        ];
        let specific = make_specific(vec![
            make_node(NodeRole::Primary, "192.168.1.10", "DSC0"),
            make_node(NodeRole::Standby, "192.168.1.11", "DSC1"),
        ]);
        let common = make_common_with_fake_installer(&dir);

        let result = super::run_with_runners(
            common,
            specific,
            runners,
            |_host, _port, _secs| Box::pin(async { Ok(()) }),
        )
        .await;

        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok(), "所有 checkpoint 均已完成，run_with_runners 应成功，实际: {:?}", result);

        let log0 = runner0.exec_log();
        let log1 = runner1.exec_log();
        let all_log: Vec<String> = log0.into_iter().chain(log1.into_iter()).collect();

        assert!(
            !all_log.iter().any(|c| c.contains("dm_service_installer.sh")),
            "已跳过阶段不应出现 dm_service_installer.sh，实际: {:?}",
            all_log
        );
        assert!(
            !all_log.iter().any(|c| c.contains("dmasmcmd")),
            "已跳过阶段不应出现 dmasmcmd，实际: {:?}",
            all_log
        );
        assert!(
            !all_log.iter().any(|c| c.contains("dmasmtool")),
            "已跳过阶段不应出现 dmasmtool，实际: {:?}",
            all_log
        );
        assert!(
            !all_log.iter().any(|c| c.contains("dminit control=")),
            "已跳过阶段不应出现 dminit control=，实际: {:?}",
            all_log
        );
    }

    // Test 2: 无 checkpoint 时按顺序调用各步骤
    #[tokio::test]
    async fn test_run_with_runners_calls_steps_in_order_no_checkpoint() {
        let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let primary = make_node(NodeRole::Primary, "192.168.1.10", "DSC0");
        let standby = make_node(NodeRole::Standby, "192.168.1.11", "DSC1");
        let runner0 = {
            let r = make_runner_with_preflight();
            // verify_dsc_node 命令以 "echo 'SELECT STATUS$" 开头，需预设含 OPEN 的响应
            // 注意：runner 的 responses 用 Vec 构造，追加需重建
            drop(r);
            let df_output = b"Filesystem     1B-blocks       Used  Available Use% Mounted on\n\
/dev/sda1    107374182400 10737418240 10737418240  50% /opt\n";
            MockRunner::new(vec![
                ("sudo -n true".to_string(), 0, vec![]),
                ("ss -tlnp | grep ':5236'".to_string(), 0, vec![]),
                ("df -B1 /opt".to_string(), 0, df_output.to_vec()),
                // verify_dsc_node 验证（primary）
                ("echo 'SELECT STATUS$".to_string(), 0, b"STATUS$   MODE$\nOPEN      NORMAL\n".to_vec()),
            ])
        };
        let runner0 = Arc::new(runner0);
        // 预设 tar.gz 读取数据，确保 distribute_config_dir 可以 sftp_read
        runner0.set_sftp_read("/tmp/dsc1_config.tar.gz", b"fake-tar-content".to_vec());
        let runner1 = {
            let df_output = b"Filesystem     1B-blocks       Used  Available Use% Mounted on\n\
/dev/sda1    107374182400 10737418240 10737418240  50% /opt\n";
            MockRunner::new(vec![
                ("sudo -n true".to_string(), 0, vec![]),
                ("ss -tlnp | grep ':5236'".to_string(), 0, vec![]),
                ("df -B1 /opt".to_string(), 0, df_output.to_vec()),
                // verify_dsc_node 验证（standby）
                ("echo 'SELECT STATUS$".to_string(), 0, b"STATUS$   MODE$\nOPEN      NORMAL\n".to_vec()),
            ])
        };
        let runner1 = Arc::new(runner1);

        let runners: phases::Runners = vec![
            (primary, Arc::clone(&runner0) as Arc<dyn crate::common::ssh::CommandRunner>),
            (standby, Arc::clone(&runner1) as Arc<dyn crate::common::ssh::CommandRunner>),
        ];
        let specific = make_specific(vec![
            make_node(NodeRole::Primary, "192.168.1.10", "DSC0"),
            make_node(NodeRole::Standby, "192.168.1.11", "DSC1"),
        ]);
        let common = make_common_with_fake_installer(&dir);

        let result = super::run_with_runners(
            common,
            specific,
            runners,
            |_host, _port, _secs| Box::pin(async { Ok(()) }),
        )
        .await;

        std::env::set_current_dir(original_dir).unwrap();

        // MockRunner 默认返回 ([], 0)，所有步骤应成功
        assert!(result.is_ok(), "run_with_runners 应成功，实际: {:?}", result);

        let log0 = runner0.exec_log();
        let log1 = runner1.exec_log();

        // runner0（Primary）应含 dmasmcmd / dmasmtool / dminit control=
        assert!(
            log0.iter().any(|c| c.contains("dmasmcmd")),
            "Primary runner 应含 dmasmcmd，log0: {:?}",
            log0
        );
        assert!(
            log0.iter().any(|c| c.contains("dmasmtool")),
            "Primary runner 应含 dmasmtool，log0: {:?}",
            log0
        );
        assert!(
            log0.iter().any(|c| c.contains("dminit control=")),
            "Primary runner 应含 dminit control=，log0: {:?}",
            log0
        );

        // dmasmcmd 必须在 dmasmtool 之前
        let dmasmcmd_pos = log0.iter().position(|c| c.contains("dmasmcmd")).unwrap();
        let dmasmtool_pos = log0.iter().position(|c| c.contains("dmasmtool")).unwrap();
        assert!(
            dmasmcmd_pos < dmasmtool_pos,
            "dmasmcmd 应在 dmasmtool 之前，log0: {:?}",
            log0
        );

        // runner0（Primary）应含 dm_service_installer.sh（DMCSS + DMASM + DMSERVER）
        assert!(
            log0.iter().any(|c| c.contains("dm_service_installer.sh") && c.contains("-t dmcss")),
            "Primary runner 应含 dm_service_installer.sh -t dmcss，log0: {:?}",
            log0
        );
        assert!(
            log0.iter().any(|c| c.contains("dm_service_installer.sh") && c.contains("-t dmasmsvr")),
            "Primary runner 应含 dm_service_installer.sh -t dmasmsvr，log0: {:?}",
            log0
        );
        assert!(
            log0.iter().any(|c| c.contains("dm_service_installer.sh") && c.contains("-t dmserver")),
            "Primary runner 应含 dm_service_installer.sh -t dmserver，log0: {:?}",
            log0
        );

        // runner0（Primary）应含 tar czf（打包 config 目录）
        assert!(
            log0.iter().any(|c| c.contains("tar czf")),
            "Primary runner 应含 tar czf，log0: {:?}",
            log0
        );

        // runner1（Standby）也应含 dm_service_installer.sh
        assert!(
            log1.iter().any(|c| c.contains("dm_service_installer.sh") && c.contains("-t dmcss")),
            "Standby runner 应含 dm_service_installer.sh -t dmcss，log1: {:?}",
            log1
        );

        // runner1（Standby）应含 tar xzf（解压 config 目录）
        assert!(
            log1.iter().any(|c| c.contains("tar xzf")),
            "Standby runner 应含 tar xzf，log1: {:?}",
            log1
        );

        // verify：两个 runner 都应含 SELECT STATUS$
        assert!(
            log0.iter().any(|c| c.contains("SELECT STATUS$")),
            "Primary runner 应含 SELECT STATUS$（verify），log0: {:?}",
            log0
        );
        assert!(
            log1.iter().any(|c| c.contains("SELECT STATUS$")),
            "Standby runner 应含 SELECT STATUS$（verify），log1: {:?}",
            log1
        );
    }

    // Test 3: first_node 是 Primary 角色（Standby 在前时仍正确）
    #[tokio::test]
    async fn test_first_node_is_primary_role() {
        let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        // Standby 在前，Primary 在后
        let standby = make_node(NodeRole::Standby, "192.168.1.11", "DSC1");
        let primary = make_node(NodeRole::Primary, "192.168.1.10", "DSC0");
        let runner_standby = Arc::new(make_runner_with_preflight());
        let runner_primary = Arc::new(make_runner_with_preflight());
        // primary runner 预设 tar.gz sftp_read 数据
        runner_primary.set_sftp_read("/tmp/dsc0_config.tar.gz", b"fake-tar-content".to_vec());

        let runners: phases::Runners = vec![
            (standby, Arc::clone(&runner_standby) as Arc<dyn crate::common::ssh::CommandRunner>),
            (primary, Arc::clone(&runner_primary) as Arc<dyn crate::common::ssh::CommandRunner>),
        ];
        let specific = make_specific(vec![
            make_node(NodeRole::Standby, "192.168.1.11", "DSC1"),
            make_node(NodeRole::Primary, "192.168.1.10", "DSC0"),
        ]);
        let common = make_common_with_fake_installer(&dir);

        let _result = super::run_with_runners(
            common,
            specific,
            runners,
            |_host, _port, _secs| Box::pin(async { Ok(()) }),
        )
        .await;

        std::env::set_current_dir(original_dir).unwrap();

        let log_primary = runner_primary.exec_log();
        let log_standby = runner_standby.exec_log();

        // dmasmcmd 应仅出现在 Primary runner 上
        assert!(
            !log_standby.iter().any(|c| c.contains("dmasmcmd")),
            "dmasmcmd 不应出现在 Standby runner，standby log: {:?}",
            log_standby
        );
        assert!(
            log_primary.iter().any(|c| c.contains("dmasmcmd")),
            "dmasmcmd 应出现在 Primary runner，primary log: {:?}",
            log_primary
        );
    }

    // Test 4: 全 standby 节点时返回错误
    #[tokio::test]
    async fn test_run_with_runners_returns_error_when_no_primary_node() {
        let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let standby1 = make_node(NodeRole::Standby, "192.168.1.10", "DSC0");
        let standby2 = make_node(NodeRole::Standby, "192.168.1.11", "DSC1");
        // 提供 preflight 响应，让流程进入 first_node_index 检查
        let runner1 = Arc::new(make_runner_with_preflight());
        let runner2 = Arc::new(make_runner_with_preflight());

        let runners: phases::Runners = vec![
            (standby1, Arc::clone(&runner1) as Arc<dyn crate::common::ssh::CommandRunner>),
            (standby2, Arc::clone(&runner2) as Arc<dyn crate::common::ssh::CommandRunner>),
        ];
        let specific = make_specific(vec![
            make_node(NodeRole::Standby, "192.168.1.10", "DSC0"),
            make_node(NodeRole::Standby, "192.168.1.11", "DSC1"),
        ]);
        let common = make_common_with_fake_installer(&dir);

        let result = super::run_with_runners(
            common,
            specific,
            runners,
            |_host, _port, _secs| Box::pin(async { Ok(()) }),
        )
        .await;

        std::env::set_current_dir(original_dir).unwrap();

        assert!(result.is_err(), "全 standby 节点应返回 Err，实际: {:?}", result);
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.to_lowercase().contains("primary") || msg.contains("first_node"),
            "错误消息应含 'primary' 或 'first_node'，实际: {}",
            msg
        );
    }

    // Test 5: checkpoint 在 dminit 失败时，前面的 gate 已保存
    #[tokio::test]
    async fn test_checkpoint_saved_after_each_phase() {
        let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let primary = make_node(NodeRole::Primary, "192.168.1.10", "DSC0");
        let standby = make_node(NodeRole::Standby, "192.168.1.11", "DSC1");

        // df 输出（preflight 需要）
        let df_output = b"Filesystem     1B-blocks       Used  Available Use% Mounted on\n\
/dev/sda1    107374182400 10737418240 10737418240  50% /opt\n";

        // 设置 Primary runner：preflight 响应 + dmasmcmd/dmasmtool 成功 + dminit 失败
        let runner0 = Arc::new(MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("ss -tlnp | grep ':5236'".to_string(), 0, vec![]),
            ("df -B1 /opt".to_string(), 0, df_output.to_vec()),
            (
                // dmasmcmd（前缀 "printf"，由 run_dmasmcmd_init 内部构造）
                "printf '%s\\n'".to_string(),
                0,
                vec![],
            ),
            (
                // dmasmtool（前缀 "printf"，由 run_dmasmtool_create_diskgroups 内部构造）
                "printf '%s\\n'".to_string(),
                0,
                vec![],
            ),
            (
                // dminit control= 失败（shell_quote 会给路径加单引号：'/opt/dmdbms'/bin/dminit）
                "'/opt/dmdbms'/bin/dminit".to_string(),
                1,
                b"mock dminit failure".to_vec(),
            ),
        ]));
        let runner1 = Arc::new(MockRunner::new(vec![
            ("sudo -n true".to_string(), 0, vec![]),
            ("ss -tlnp | grep ':5236'".to_string(), 0, vec![]),
            ("df -B1 /opt".to_string(), 0, df_output.to_vec()),
        ]));

        let runners: phases::Runners = vec![
            (primary, Arc::clone(&runner0) as Arc<dyn crate::common::ssh::CommandRunner>),
            (standby, Arc::clone(&runner1) as Arc<dyn crate::common::ssh::CommandRunner>),
        ];
        let specific = make_specific(vec![
            make_node(NodeRole::Primary, "192.168.1.10", "DSC0"),
            make_node(NodeRole::Standby, "192.168.1.11", "DSC1"),
        ]);
        let common = make_common_with_fake_installer(&dir);

        let result = super::run_with_runners(
            common,
            specific,
            runners,
            |_host, _port, _secs| Box::pin(async { Ok(()) }),
        )
        .await;

        std::env::set_current_dir(original_dir).unwrap();

        // run_with_runners 应当失败
        assert!(result.is_err(), "dminit 失败应导致 run_with_runners 返回 Err，实际: {:?}", result);

        // 验证 checkpoint 文件中 css_asm_started=true, asm_diskgroup_created=true, dminit_shared_done=false
        let saved_cp = crate::cluster::checkpoint::ClusterCheckpoint::load_from(dir.path())
            .unwrap()
            .expect("checkpoint 文件应存在");
        assert!(
            saved_cp.css_asm_started,
            "css_asm_started 应为 true，checkpoint: {:?}",
            saved_cp
        );
        assert!(
            saved_cp.asm_diskgroup_created,
            "asm_diskgroup_created 应为 true，checkpoint: {:?}",
            saved_cp
        );
        assert!(
            !saved_cp.dminit_shared_done,
            "dminit_shared_done 应为 false，checkpoint: {:?}",
            saved_cp
        );
    }
}
