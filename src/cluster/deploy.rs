use anyhow::{Context, Result};
use std::path::Path;

use crate::cluster::ssh::CommandRunner;
use crate::cluster::templates::{
    generate_dm_ini_cluster_suffix, generate_dmarch_ini, generate_dmmal_ini, generate_dmwatcher_ini,
};
use crate::config::cluster::{NodeConfig, NodeRole};
use crate::config::InstallConfig;
use crate::install::silent_install::generate_install_xml;

/// 构建 dminit 命令行参数列表（等号两侧无空格，防止 Pitfall 2）。
pub fn build_dminit_args(node: &NodeConfig) -> Vec<String> {
    vec![
        format!("{}/bin/dminit", node.install_path),
        format!("PATH={}", node.data_path),
        format!("INSTANCE_NAME={}", node.instance_name),
        format!("PORT_NUM={}", node.port),
        format!("PAGE_SIZE={}", node.page_size),
        format!("CHARSET={}", node.charset),
        format!("CASE_SENSITIVE={}", if node.case_sensitive { 1 } else { 0 }),
        format!("EXTENT_SIZE={}", node.extent_size),
    ]
}

/// 上传安装包 + XML response file，执行远端静默安装。
pub async fn upload_installer_and_install(
    node: &NodeConfig,
    package_path: &Path,
    runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("[node:{:?}][1/6] 生成 XML response file", node.role);
    let install_config = node_to_install_config(node);
    let xml_file = generate_install_xml(&install_config).context("生成 XML response file 失败")?;
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
    let remote_iso = format!("/tmp/dm_installer_{}.iso", node.instance_name);
    runner
        .sftp_write(&remote_iso, &bytes)
        .await
        .context("SFTP 上传安装包失败")?;
    let install_cmd = format!("cd /tmp && DMInstall.bin -q {}", remote_xml);
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

/// 将 NodeConfig 映射为 InstallConfig（用于 XML 生成）。
fn node_to_install_config(node: &NodeConfig) -> InstallConfig {
    InstallConfig {
        install_path: node.install_path.clone(),
        data_path: node.data_path.clone(),
        instance_name: node.instance_name.clone(),
        port: node.port,
        page_size: node.page_size,
        charset: node.charset,
        case_sensitive: node.case_sensitive,
        extent_size: node.extent_size,
    }
}

/// 远端执行 dminit 初始化数据库。
pub async fn run_dminit_remote(node: &NodeConfig, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[node:{:?}][3/6] 执行 dminit", node.role);
    let cmd = build_dminit_args(node).join(" ");
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

/// 计算远端配置文件目标路径。
fn target_path(node: &NodeConfig, filename: &str) -> String {
    format!("{}/{}/{}", node.data_path, node.instance_name, filename)
}

/// 分发 4 个 INI 配置文件到远端节点。
pub async fn distribute_configs(
    node: &NodeConfig,
    all_nodes: &[NodeConfig],
    oguid: u32,
    runner: &dyn CommandRunner,
) -> Result<()> {
    tracing::info!("[node:{:?}][4/6] 分发配置文件", node.role);
    let peer = all_nodes
        .iter()
        .find(|n| n.instance_name != node.instance_name)
        .context("找不到对端节点")?;
    let dm_ini_suffix = generate_dm_ini_cluster_suffix(node);
    let dmmal_ini = generate_dmmal_ini(all_nodes);
    let dmarch_ini = generate_dmarch_ini(node, &peer.instance_name);
    let dmwatcher_ini = generate_dmwatcher_ini(node, oguid);
    runner
        .sftp_write(&target_path(node, "dm.ini.cluster_suffix"), dm_ini_suffix.as_bytes())
        .await
        .context("SFTP 上传 dm.ini.cluster_suffix 失败")?;
    runner
        .sftp_write(&target_path(node, "dmmal.ini"), dmmal_ini.as_bytes())
        .await
        .context("SFTP 上传 dmmal.ini 失败")?;
    runner
        .sftp_write(&target_path(node, "dmarch.ini"), dmarch_ini.as_bytes())
        .await
        .context("SFTP 上传 dmarch.ini 失败")?;
    runner
        .sftp_write(&target_path(node, "dmwatcher.ini"), dmwatcher_ini.as_bytes())
        .await
        .context("SFTP 上传 dmwatcher.ini 失败")?;
    let merge_cmd = format!(
        "cat {0} >> {1}",
        target_path(node, "dm.ini.cluster_suffix"),
        target_path(node, "dm.ini")
    );
    runner
        .exec(&merge_cmd)
        .await
        .map_err(|e| anyhow::anyhow!("合并 dm.ini 失败: {}", e))?;
    Ok(())
}

/// 以 mount 模式启动 dmserver（后台 nohup，Pitfall 4）。
pub async fn start_dmserver_mount(node: &NodeConfig, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[node:{:?}][5/6] mount 模式启动 dmserver", node.role);
    let cmd = format!(
        "nohup {0}/bin/dmserver {1}/{2}/dm.ini mount > /tmp/dmserver_{2}.log 2>&1 &",
        node.install_path, node.data_path, node.instance_name
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
    role: NodeRole,
    oguid: u32,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let role_sql = match role {
        NodeRole::Primary => "alter database primary;",
        NodeRole::Standby => "alter database standby;",
    };
    let sql_block = format!(
        "SP_SET_PARA_VALUE(1,'ALTER_MODE_STATUS',1);sp_set_oguid({oguid});{role_sql}SP_SET_PARA_VALUE(1,'ALTER_MODE_STATUS',0);"
    );
    let cmd = format!(
        "echo \"{}\" | {}/bin/disql SYSDBA/SYSDBA@localhost:{}",
        sql_block, node.install_path, node.port
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

/// 启动 dmwatcher 守护进程（后台 nohup）。
pub async fn start_dmwatcher(node: &NodeConfig, runner: &dyn CommandRunner) -> Result<()> {
    tracing::info!("[node:{:?}][6/6] 启动 dmwatcher", node.role);
    let cmd = format!(
        "nohup {0}/bin/dmwatcher {1}/{2}/dmwatcher.ini > /tmp/dmwatcher_{2}.log 2>&1 &",
        node.install_path, node.data_path, node.instance_name
    );
    runner
        .exec(&cmd)
        .await
        .map_err(|e| anyhow::anyhow!("启动 dmwatcher 失败: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::ssh::MockRunner;
    use crate::config::cluster::{NodeConfig, NodeRole, SshCredentials};

    fn make_primary_node() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Primary,
            host: "192.168.1.10".to_string(),
            port: 5236,
            instance_name: "DMSVR01".to_string(),
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            page_size: 8,
            charset: 0,
            case_sensitive: true,
            extent_size: 16,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: None,
                password: Some("pass".to_string()),
            },
        }
    }

    fn make_standby_node() -> NodeConfig {
        NodeConfig {
            role: NodeRole::Standby,
            host: "192.168.1.11".to_string(),
            port: 5236,
            instance_name: "DMSVR02".to_string(),
            install_path: "/opt/dmdbms".to_string(),
            data_path: "/opt/dmdbms/data".to_string(),
            mal_port: 5237,
            dw_port: 5238,
            inst_dw_port: 5239,
            page_size: 8,
            charset: 0,
            case_sensitive: true,
            extent_size: 16,
            ssh: SshCredentials {
                user: "root".to_string(),
                identity_file: None,
                password: Some("pass".to_string()),
            },
        }
    }

    #[test]
    fn test_build_dminit_args_format() {
        let node = make_primary_node();
        let args = build_dminit_args(&node);
        assert_eq!(args[0], "/opt/dmdbms/bin/dminit", "第一项应为 dminit 路径");
        assert!(args.contains(&"PATH=/opt/dmdbms/data".to_string()), "应含 PATH=");
        assert!(args.contains(&"INSTANCE_NAME=DMSVR01".to_string()), "应含 INSTANCE_NAME=");
        assert!(args.contains(&"PORT_NUM=5236".to_string()), "应含 PORT_NUM=（无空格）");
        assert!(args.contains(&"PAGE_SIZE=8".to_string()), "应含 PAGE_SIZE=");
    }

    #[tokio::test]
    async fn test_distribute_configs_calls_four_sftp_writes() {
        let primary = make_primary_node();
        let standby = make_standby_node();
        let all_nodes = vec![primary.clone(), standby.clone()];
        let runner = MockRunner::new(vec![]);
        distribute_configs(&primary, &all_nodes, 453331, &runner)
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
        let runner = MockRunner::new(vec![]);
        configure_database_role(&node, NodeRole::Primary, 453331, &runner)
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
        let runner = MockRunner::new(vec![]);
        configure_database_role(&node, NodeRole::Standby, 453331, &runner)
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
        let runner = MockRunner::new(vec![]);
        start_dmserver_mount(&node, &runner).await.unwrap();
        let log = runner.exec_log();
        let found = log.iter().any(|cmd| {
            cmd.contains("dmserver") && cmd.contains("mount") && cmd.contains("nohup")
        });
        assert!(found, "命令应含 dmserver/mount/nohup: {:?}", log);
    }

    #[tokio::test]
    async fn test_upload_installer_and_install_pushes_xml() {
        let node = make_primary_node();
        let runner = MockRunner::new(vec![]);
        // 使用不存在的路径——upload 会因无法读取 ISO 失败，但 XML 应已推送
        let pkg = std::path::PathBuf::from("/tmp/fake_nonexistent.iso");
        // XML 推送发生在 ISO 读取之前，所以这里先检查 sftp_log 在失败前
        // 实际上 tokio::fs::read 失败会返回 Err 在 sftp_write xml 之后
        // 测试：传入一个临时文件作为"安装包"
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let result = upload_installer_and_install(&node, tmp.path(), &runner).await;
        // 即使 install cmd 可能因 exec 默认 Ok 通过，sftp_log 应含 xml 和 iso
        let _ = result; // 结果可能 Ok 也可能因 dminit 等原因 Err，主要验证 sftp
        let log = runner.sftp_log();
        let has_xml = log.iter().any(|(p, _)| p.contains(".xml"));
        let has_iso = log.iter().any(|(p, _)| p.contains(".iso"));
        assert!(has_xml, "sftp_log 应含 .xml 路径: {:?}", log.iter().map(|(p,_)| p).collect::<Vec<_>>());
        assert!(has_iso, "sftp_log 应含 .iso 路径: {:?}", log.iter().map(|(p,_)| p).collect::<Vec<_>>());
    }
}
