use futures::future::join_all;

use crate::cluster::phases::Runners;
use crate::config::cluster::DminitConfig;

#[derive(Default)]
pub struct ClusterRollbackState {
    pub install_done: bool,
    pub primary_inited: bool,
    pub standbys_restored: bool,
    pub services_started: bool,
    pub watchers_started: bool,
    pub monitors_started: bool,
}

pub async fn run(state: &ClusterRollbackState, runners: &Runners, dminit: &DminitConfig) {
    tracing::warn!("集群部署失败，开始回退...");
    if state.monitors_started {
        stop_monitors(runners).await;
    }
    if state.watchers_started {
        stop_watchers(runners, dminit).await;
    }
    if state.services_started {
        stop_services(runners, dminit).await;
    }
    if state.standbys_restored || state.primary_inited {
        clean_data(runners, dminit).await;
    }
    if state.install_done {
        clean_install(runners, dminit).await;
    }
    tracing::warn!("集群回退完成");
}

async fn stop_monitors(runners: &Runners) {
    let futs = runners.iter().map(|(node, runner)| {
        let runner = runner.clone();
        let host = node.host.clone();
        async move {
            let _ = runner.exec(
                "systemctl stop DmMonitorService 2>/dev/null || true; \
                 systemctl disable DmMonitorService 2>/dev/null || true; \
                 rm -f /etc/systemd/system/DmMonitorService.service \
                       /usr/lib/systemd/system/DmMonitorService.service 2>/dev/null || true; \
                 systemctl daemon-reload 2>/dev/null || true",
            ).await;
            tracing::info!("[回退] {host} 已停止 DmMonitorService");
        }
    });
    join_all(futs).await;
}

async fn stop_watchers(runners: &Runners, dminit: &DminitConfig) {
    let install_path = &dminit.install_path;
    let futs = runners.iter().map(|(node, runner)| {
        let runner = runner.clone();
        let host = node.host.clone();
        let path = install_path.clone();
        async move {
            let _ = runner.exec(&format!(
                "systemctl stop DmWatcherService 2>/dev/null || true; \
                 systemctl disable DmWatcherService 2>/dev/null || true; \
                 rm -f /etc/systemd/system/DmWatcherService.service \
                       /usr/lib/systemd/system/DmWatcherService.service 2>/dev/null || true; \
                 systemctl daemon-reload 2>/dev/null || true; \
                 pkill -f '{path}/bin/dmwatcher' 2>/dev/null || true",
            )).await;
            tracing::info!("[回退] {host} 已停止 dmwatcher");
        }
    });
    join_all(futs).await;
}

async fn stop_services(runners: &Runners, dminit: &DminitConfig) {
    let install_path = &dminit.install_path;
    let futs = runners.iter().map(|(node, runner)| {
        let runner = runner.clone();
        let host = node.host.clone();
        let svc = format!("DmService{}", node.instance_name);
        let path = install_path.clone();
        async move {
            let _ = runner.exec(&format!(
                "systemctl stop '{svc}' DmAPService 2>/dev/null || true; \
                 systemctl disable '{svc}' DmAPService 2>/dev/null || true; \
                 rm -f /etc/systemd/system/{svc}.service \
                       /etc/systemd/system/DmAPService.service \
                       /usr/lib/systemd/system/{svc}.service \
                       /usr/lib/systemd/system/DmAPService.service 2>/dev/null || true; \
                 systemctl daemon-reload 2>/dev/null || true; \
                 pkill -f '{path}/bin/dmserver' 2>/dev/null || true",
            )).await;
            tracing::info!("[回退] {host} 已停止 dmserver 服务");
        }
    });
    join_all(futs).await;
}

async fn clean_data(runners: &Runners, dminit: &DminitConfig) {
    let data_path = &dminit.data_path;
    let futs = runners.iter().map(|(node, runner)| {
        let runner = runner.clone();
        let host = node.host.clone();
        let path = data_path.clone();
        async move {
            let _ = runner.exec(&format!("rm -rf '{path}'")).await;
            tracing::info!("[回退] {host} 已删除数据目录: {path}");
        }
    });
    join_all(futs).await;
}

async fn clean_install(runners: &Runners, dminit: &DminitConfig) {
    let install_path = &dminit.install_path;
    let futs = runners.iter().map(|(node, runner)| {
        let runner = runner.clone();
        let host = node.host.clone();
        let path = install_path.clone();
        async move {
            let _ = runner.exec(&format!("rm -rf '{path}'")).await;
            tracing::info!("[回退] {host} 已删除安装目录: {path}");
        }
    });
    join_all(futs).await;
}
