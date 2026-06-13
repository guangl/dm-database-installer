/// 当前平台信息，用于匹配 versions.txt 条目。
#[derive(Debug, Clone)]
pub struct Platform {
    /// 对应 versions.txt 第一列，如 "x86_64" / "aarch64"
    pub arch: String,
    /// 对应 versions.txt 第二列，如 "x86" / "hygon" / "kunpeng"（无法检测时为 None）
    pub cpu: Option<String>,
    /// 对应 versions.txt 第三列，如 "rhel7" / "kylin10_sp3"（无法检测时为 None）
    pub os: Option<String>,
}

/// 检测当前平台。在非 Linux 系统上 cpu/os 均返回 None。
pub fn detect_platform() -> Platform {
    let arch = map_arch(std::env::consts::ARCH);
    let cpu = detect_cpu(&arch);
    let os = detect_os();
    Platform { arch, cpu, os }
}

fn map_arch(rust_arch: &str) -> String {
    match rust_arch {
        "x86_64"     => "x86_64".into(),
        "aarch64"    => "aarch64".into(),
        "mips64"     => "mips64el".into(),
        "loongarch64"=> "loongarch64".into(),
        other        => other.into(),
    }
}

fn detect_cpu(arch: &str) -> Option<String> {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    match arch {
        "x86_64" => {
            if cpuinfo.contains("HygonGenuine") {
                Some("hygon".into())
            } else {
                Some("x86".into())
            }
        }
        "aarch64" => {
            let lower = cpuinfo.to_lowercase();
            if lower.contains("kunpeng") || lower.contains("hisilicon") || cpuinfo.contains("0x48") {
                Some("kunpeng".into())
            } else if lower.contains("phytium") || cpuinfo.contains("0x70") {
                Some("ft2000".into())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn detect_os() -> Option<String> {
    let content = std::fs::read_to_string("/etc/os-release").ok()?;
    let mut id = None;
    let mut version_id = None;

    for line in content.lines() {
        if let Some(v) = line.strip_prefix("ID=") {
            id = Some(v.trim_matches('"').to_lowercase());
        } else if let Some(v) = line.strip_prefix("VERSION_ID=") {
            version_id = Some(v.trim_matches('"').to_lowercase());
        }
    }

    map_os_key(id.as_deref()?, version_id.as_deref())
}

fn map_os_key(id: &str, version_id: Option<&str>) -> Option<String> {
    let ver = version_id.unwrap_or("");
    match id {
        "rhel" | "redhat" => {
            if ver.starts_with('7') { Some("rhel7".into()) }
            else if ver.starts_with('6') { Some("rhel6".into()) }
            else { None }
        }
        "centos" => {
            if ver.starts_with('7') { Some("centos7".into()) }
            else { None }
        }
        "kylin" => {
            let upper = ver.to_uppercase();
            if upper.contains("SP3") { Some("kylin10_sp3".into()) }
            else if upper.contains("SP1") { Some("kylin10_sp1".into()) }
            else { Some("kylin10".into()) }
        }
        "ubuntu" => {
            if ver.starts_with("22.") { Some("ubuntu22".into()) }
            else { None }
        }
        "uos"  => Some("uos20".into()),
        "nfsc" => Some("nfsc".into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_arch_known_values() {
        assert_eq!(map_arch("x86_64"), "x86_64");
        assert_eq!(map_arch("aarch64"), "aarch64");
        assert_eq!(map_arch("mips64"), "mips64el");
        assert_eq!(map_arch("loongarch64"), "loongarch64");
    }

    #[test]
    fn test_map_arch_passthrough() {
        assert_eq!(map_arch("riscv64"), "riscv64");
    }

    #[test]
    fn test_map_os_rhel7() {
        assert_eq!(map_os_key("rhel", Some("7.9")), Some("rhel7".into()));
    }

    #[test]
    fn test_map_os_rhel6() {
        assert_eq!(map_os_key("redhat", Some("6.10")), Some("rhel6".into()));
    }

    #[test]
    fn test_map_os_centos7() {
        assert_eq!(map_os_key("centos", Some("7")), Some("centos7".into()));
    }

    #[test]
    fn test_map_os_kylin_sp3() {
        assert_eq!(map_os_key("kylin", Some("V10SP3")), Some("kylin10_sp3".into()));
    }

    #[test]
    fn test_map_os_kylin_sp1() {
        assert_eq!(map_os_key("kylin", Some("V10 SP1")), Some("kylin10_sp1".into()));
    }

    #[test]
    fn test_map_os_kylin_no_sp() {
        assert_eq!(map_os_key("kylin", Some("V10")), Some("kylin10".into()));
    }

    #[test]
    fn test_map_os_ubuntu22() {
        assert_eq!(map_os_key("ubuntu", Some("22.04")), Some("ubuntu22".into()));
    }

    #[test]
    fn test_map_os_uos() {
        assert_eq!(map_os_key("uos", None), Some("uos20".into()));
    }

    #[test]
    fn test_map_os_unknown_returns_none() {
        assert_eq!(map_os_key("arch", Some("rolling")), None);
    }
}
