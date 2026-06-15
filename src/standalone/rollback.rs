use std::path::Path;
use std::process::Command;

const BACKUP_FILES: &[(&str, &str)] = &[
    ("/etc/sysctl.conf",           "sysctl.conf"),
    ("/etc/security/limits.conf",  "limits.conf"),
    ("/etc/selinux/config",        "selinux_config"),
    ("/etc/ssh/sshd_config",       "sshd_config"),
    ("/etc/pam.d/login",           "pam_login"),
    ("/etc/rc.local",              "rc.local"),
    ("/etc/profile",               "profile"),
];

/// 安装前系统文件快照；drop 时自动清理临时目录。
pub struct EnvBackup {
    dir: tempfile::TempDir,
}

impl EnvBackup {
    pub fn capture() -> anyhow::Result<Self> {
        let dir = tempfile::TempDir::new()?;
        for (src, name) in BACKUP_FILES {
            if Path::new(src).exists() {
                let _ = std::fs::copy(src, dir.path().join(name));
            }
        }
        save_thp(dir.path());
        save_runtime_state(dir.path());
        Ok(Self { dir })
    }

    pub fn restore(&self) {
        for (dst, name) in BACKUP_FILES {
            let src = self.dir.path().join(name);
            if src.exists() {
                if let Err(e) = std::fs::copy(&src, dst) {
                    tracing::warn!("[回退] 恢复 {dst} 失败: {e}");
                } else {
                    tracing::info!("[回退] 已恢复: {dst}");
                }
            }
        }
        restore_derived(self.dir.path());
    }
}

fn save_thp(dir: &Path) {
    let thp = "/sys/kernel/mm/transparent_hugepage/enabled";
    if let Ok(content) = std::fs::read_to_string(thp) {
        let val = content.split_whitespace()
            .find(|s| s.starts_with('[') && s.ends_with(']'))
            .map(|s| s.trim_matches(|c| c == '[' || c == ']'))
            .unwrap_or("always");
        let _ = std::fs::write(dir.join("thp"), val);
    }
}

fn save_runtime_state(dir: &Path) {
    let fw_active = Command::new("systemctl")
        .args(["is-active", "firewalld"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false);
    let _ = std::fs::write(dir.join("firewall"), if fw_active { "active" } else { "inactive" });

    if let Ok(o) = Command::new("timedatectl").args(["show", "--property=Timezone", "--value"]).output() {
        let _ = std::fs::write(dir.join("timezone"), &o.stdout);
    }
    if let Ok(o) = Command::new("getenforce").output() {
        let _ = std::fs::write(dir.join("selinux_mode"), &o.stdout);
    }
}

fn restore_derived(dir: &Path) {
    let _ = Command::new("sysctl").arg("-p").status();
    let _ = Command::new("systemctl").args(["reload", "sshd"]).status();

    let thp = "/sys/kernel/mm/transparent_hugepage/enabled";
    if Path::new(thp).exists() {
        if let Ok(val) = std::fs::read_to_string(dir.join("thp")) {
            if let Err(e) = std::fs::write(thp, val.trim()) {
                tracing::warn!("[回退] 恢复 THP 失败: {e}");
            } else {
                tracing::info!("[回退] 已恢复 THP");
            }
        }
    }
    if let Ok(state) = std::fs::read_to_string(dir.join("firewall")) {
        if state.trim() == "active" {
            let _ = Command::new("systemctl").args(["enable", "firewalld"]).status();
            let _ = Command::new("systemctl").args(["start",  "firewalld"]).status();
            tracing::info!("[回退] 已恢复 firewalld");
        }
    }
    if let Ok(tz) = std::fs::read_to_string(dir.join("timezone")) {
        let tz = tz.trim();
        if !tz.is_empty() {
            let _ = Command::new("timedatectl").args(["set-timezone", tz]).status();
            tracing::info!("[回退] 已恢复时区: {tz}");
        }
    }
    if let Ok(mode) = std::fs::read_to_string(dir.join("selinux_mode")) {
        match mode.trim() {
            "Enforcing"  => { let _ = Command::new("setenforce").arg("1").status(); tracing::info!("[回退] 已恢复 SELinux: Enforcing"); }
            "Permissive" => { let _ = Command::new("setenforce").arg("0").status(); }
            _ => {}
        }
    }
}

/// RAII 回退守卫：安装失败时 Drop 自动清理，成功时调用 commit() 阻止清理。
pub struct StandaloneRollback {
    env_backup: Option<EnvBackup>,
    install_path: String,
    data_path: String,
    instance_name: String,
    pub installed: bool,
    pub db_inited: bool,
    pub services_registered: bool,
    committed: bool,
}

impl StandaloneRollback {
    pub fn new(install_path: &str, data_path: &str, instance_name: &str) -> Self {
        Self {
            env_backup: None,
            install_path: install_path.to_string(),
            data_path: data_path.to_string(),
            instance_name: instance_name.to_string(),
            installed: false,
            db_inited: false,
            services_registered: false,
            committed: false,
        }
    }

    pub fn set_env_backup(&mut self, backup: EnvBackup) {
        self.env_backup = Some(backup);
    }

    pub fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for StandaloneRollback {
    fn drop(&mut self) {
        if self.committed { return; }
        tracing::warn!("安装失败，开始回退...");
        rollback_services(&self.instance_name, self.services_registered);
        rollback_dm_files(&self.install_path, &self.data_path, self.installed, self.db_inited);
        if let Some(backup) = &self.env_backup {
            backup.restore();
        }
        tracing::warn!("回退完成");
    }
}

fn rollback_services(instance_name: &str, registered: bool) {
    if !registered { return; }
    let svc = format!("DmService{instance_name}");
    for name in [svc.as_str(), "DmAPService"] {
        let _ = Command::new("systemctl").args(["stop",    name]).status();
        let _ = Command::new("systemctl").args(["disable", name]).status();
        for dir in ["/etc/systemd/system", "/usr/lib/systemd/system"] {
            let path = format!("{dir}/{name}.service");
            if Path::new(&path).exists() {
                let _ = std::fs::remove_file(&path);
                tracing::info!("[回退] 已移除服务文件: {path}");
            }
        }
    }
    let _ = Command::new("systemctl").arg("daemon-reload").status();
}

fn rollback_dm_files(install_path: &str, data_path: &str, installed: bool, db_inited: bool) {
    if db_inited {
        let _ = std::fs::remove_dir_all(data_path);
        tracing::info!("[回退] 已删除数据目录: {data_path}");
    }
    if installed {
        let _ = std::fs::remove_dir_all(install_path);
        tracing::info!("[回退] 已删除安装目录: {install_path}");
    }
}
