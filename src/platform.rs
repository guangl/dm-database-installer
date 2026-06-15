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

/// 从远端 SSH 输出构建平台信息。
/// uname_m: `uname -m` 输出；cpuinfo: `/proc/cpuinfo` 内容；os_release: `/etc/os-release` 内容。
pub fn detect_platform_from_raw(uname_m: &str, cpuinfo: &str, os_release: &str) -> Platform {
    let arch = map_arch(uname_m.trim());
    let cpu = detect_cpu_from_str(&arch, cpuinfo);
    let os = map_os_key_from_str(os_release);
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
    detect_cpu_from_str(arch, &cpuinfo)
}

fn detect_cpu_from_str(arch: &str, cpuinfo: &str) -> Option<String> {
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
            // 用 "CPU implementer" 行精确匹配，避免 0x48/0x70 误中其它十六进制串
            let is_kunpeng = lower.contains("kunpeng")
                || lower.contains("hisilicon")
                || cpuinfo.contains("CPU implementer\t: 0x48")
                || cpuinfo.contains("CPU implementer : 0x48");
            let is_ft2000 = lower.contains("phytium")
                || cpuinfo.contains("CPU implementer\t: 0x70")
                || cpuinfo.contains("CPU implementer : 0x70");
            if is_kunpeng {
                Some("kunpeng".into())
            } else if is_ft2000 {
                Some("ft2000".into())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn detect_os() -> Option<String> {
    // 优先 /etc/os-release（标准路径）
    if let Some(os) = detect_os_from_os_release() {
        return Some(os);
    }
    // 各发行版自有 release 文件兜底
    detect_os_from_release_file()
}

fn detect_os_from_os_release() -> Option<String> {
    let content = std::fs::read_to_string("/etc/os-release").ok()?;
    map_os_key_from_str(&content)
}

fn map_os_key_from_str(content: &str) -> Option<String> {
    let mut id = None;
    let mut version_id = None;
    let mut pretty_name = String::new();
    let mut version = String::new();
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("ID=") {
            id = Some(v.trim_matches('"').to_lowercase());
        } else if let Some(v) = line.strip_prefix("VERSION_ID=") {
            version_id = Some(v.trim_matches('"').to_lowercase());
        } else if let Some(v) = line.strip_prefix("PRETTY_NAME=") {
            pretty_name = v.trim_matches('"').to_lowercase();
        } else if let Some(v) = line.strip_prefix("VERSION=") {
            version = v.trim_matches('"').to_lowercase();
        }
    }
    // 当 VERSION_ID 不含 SP 信息时，从 PRETTY_NAME/VERSION 补充判断（如 Kylin V10 Lance = SP1）
    if id.as_deref() == Some("kylin") {
        let extra = format!("{} {}", pretty_name, version);
        if extra.contains("sp3") {
            return Some("kylin10_sp3".into());
        }
        if extra.contains("sp1") || extra.contains("lance") {
            return Some("kylin10_sp1".into());
        }
    }
    map_os_key(id.as_deref()?, version_id.as_deref())
}

/// 模拟 `cat /etc/*-release`：枚举所有 `/etc/*-release` 文件并逐一解析。
fn detect_os_from_release_file() -> Option<String> {
    let mut paths: Vec<_> = std::fs::read_dir("/etc")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with("-release") || n == "release")
                .unwrap_or(false)
        })
        .collect();
    paths.sort(); // 排序保证行为确定
    for path in &paths {
        if let Some(os) = parse_release_file(path.to_str().unwrap_or("")) {
            tracing::debug!("OS 检测来源: {} -> {}", path.display(), os);
            return Some(os);
        }
    }
    None
}

/// 从 "Kylin Linux Advanced Server release V10 (Lance)" 之类的单行文本推断 OS key。
fn parse_release_file(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let lower = content.to_lowercase();
    let upper = content.to_uppercase();
    if lower.contains("kylin") {
        if upper.contains("SP3") {
            return Some("kylin10_sp3".into());
        }
        // SP1 的代号为 Lance，release 文件中可能只有代号而无 "SP1" 字样
        if upper.contains("SP1") || lower.contains("(lance)") {
            return Some("kylin10_sp1".into());
        }
        return Some("kylin10".into());
    }
    if lower.contains("centos") && upper.contains("RELEASE 7") {
        return Some("centos7".into());
    }
    if lower.contains("red hat") || lower.contains("redhat") {
        if upper.contains("RELEASE 7") {
            return Some("rhel7".into());
        }
        if upper.contains("RELEASE 6") {
            return Some("rhel6".into());
        }
    }
    None
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

    #[test]
    fn test_map_os_key_from_str_kylin_lance_in_pretty_name() {
        let content = "ID=kylin\nVERSION_ID=\"V10\"\nPRETTY_NAME=\"Kylin Linux Advanced Server V10 (Lance)\"\n";
        assert_eq!(
            map_os_key_from_str(content),
            Some("kylin10_sp1".into()),
            "PRETTY_NAME 含 Lance 应识别为 SP1"
        );
    }

    #[test]
    fn test_map_os_key_from_str_kylin_sp1_in_version() {
        let content = "ID=kylin\nVERSION_ID=\"V10\"\nVERSION=\"V10 SP1 (Lance)\"\n";
        assert_eq!(
            map_os_key_from_str(content),
            Some("kylin10_sp1".into()),
            "VERSION 含 SP1 应识别为 SP1"
        );
    }

    #[test]
    fn test_parse_release_file_kylin_v10_lance_is_sp1() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(
            &mut tmp.as_file(),
            b"Kylin Linux Advanced Server release V10 (Lance)\n",
        )
        .unwrap();
        let result = parse_release_file(tmp.path().to_str().unwrap());
        assert_eq!(result, Some("kylin10_sp1".into()), "Lance 是 Kylin V10 SP1 代号");
    }

    #[test]
    fn test_parse_release_file_kylin_sp1() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(
            &mut tmp.as_file(),
            b"Kylin Linux Advanced Server release V10 SP1\n",
        )
        .unwrap();
        let result = parse_release_file(tmp.path().to_str().unwrap());
        assert_eq!(result, Some("kylin10_sp1".into()));
    }

    #[test]
    fn test_parse_release_file_centos7() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(
            &mut tmp.as_file(),
            b"CentOS Linux release 7.9.2009 (Core)\n",
        )
        .unwrap();
        let result = parse_release_file(tmp.path().to_str().unwrap());
        assert_eq!(result, Some("centos7".into()));
    }

    #[test]
    fn test_parse_release_file_nonexistent_returns_none() {
        assert_eq!(parse_release_file("/nonexistent/path/release"), None);
    }
}
