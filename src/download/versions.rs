/// 内嵌 versions.txt，构建时确定，运行时无需文件系统访问。
const VERSIONS_TXT: &str = include_str!("../../versions.txt");

#[derive(Debug, Clone)]
pub struct VersionEntry {
    pub arch: String,
    pub cpu: String,
    pub os: String,
    pub url: String,
    /// SHA-256 十六进制字符串；`-` 或缺失时为 `None`。
    pub sha256: Option<String>,
}

impl VersionEntry {
    /// 从 URL 提取文件名（用于展示）。
    pub fn file_name(&self) -> &str {
        self.url.split('/').next_back().unwrap_or(&self.url)
    }
}

/// 解析内嵌的 versions.txt，跳过注释行和空行。
pub fn parse_versions() -> Vec<VersionEntry> {
    VERSIONS_TXT
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(parse_line)
        .collect()
}

fn parse_line(line: &str) -> Option<VersionEntry> {
    let mut cols = line.splitn(5, '\t');
    let arch = cols.next()?.trim().to_string();
    let cpu = cols.next()?.trim().to_string();
    let os = cols.next()?.trim().to_string();
    let url = cols.next()?.trim().to_string();
    if url.is_empty() {
        return None;
    }
    let sha256 = cols
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "-");
    Some(VersionEntry {
        arch,
        cpu,
        os,
        url,
        sha256,
    })
}

/// 按 arch/cpu/os 过滤条目。`None` 表示该维度不限制。
pub fn filter_entries<'a>(
    entries: &'a [VersionEntry],
    arch: &str,
    cpu: Option<&str>,
    os: Option<&str>,
) -> Vec<&'a VersionEntry> {
    entries
        .iter()
        .filter(|e| e.arch == arch)
        .filter(|e| cpu.is_none_or(|c| e.cpu == c))
        .filter(|e| os.is_none_or(|o| e.os == o))
        .collect()
}

/// OS 前缀回退过滤：用于 "kylin10" 无精确匹配时自动降级到 "kylin10_sp1"/"kylin10_sp3"。
/// 仅 os 维度改为前缀匹配，arch/cpu 仍精确匹配。
pub fn filter_entries_os_prefix<'a>(
    entries: &'a [VersionEntry],
    arch: &str,
    cpu: Option<&str>,
    os_prefix: &str,
) -> Vec<&'a VersionEntry> {
    entries
        .iter()
        .filter(|e| e.arch == arch)
        .filter(|e| cpu.is_none_or(|c| e.cpu == c))
        .filter(|e| e.os.starts_with(os_prefix))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_versions_not_empty() {
        let entries = parse_versions();
        assert!(!entries.is_empty(), "versions.txt 应含至少一条记录");
    }

    #[test]
    fn test_parse_versions_all_have_url() {
        for entry in parse_versions() {
            assert!(
                entry.url.starts_with("https://"),
                "URL 应以 https:// 开头: {}",
                entry.url
            );
        }
    }

    #[test]
    fn test_filter_by_arch_x86() {
        let entries = parse_versions();
        let matched = filter_entries(&entries, "x86_64", None, None);
        assert!(!matched.is_empty(), "x86_64 应有匹配条目");
        assert!(matched.iter().all(|e| e.arch == "x86_64"));
    }

    #[test]
    fn test_filter_by_arch_and_cpu() {
        let entries = parse_versions();
        let matched = filter_entries(&entries, "x86_64", Some("x86"), None);
        assert!(!matched.is_empty(), "x86_64/x86 应有匹配条目");
        assert!(matched.iter().all(|e| e.cpu == "x86"));
    }

    #[test]
    fn test_filter_by_all_fields() {
        let entries = parse_versions();
        let matched = filter_entries(&entries, "x86_64", Some("x86"), Some("rhel7"));
        assert_eq!(matched.len(), 1, "x86_64/x86/rhel7 应恰好一条");
    }

    #[test]
    fn test_filter_no_match_returns_empty() {
        let entries = parse_versions();
        let matched = filter_entries(&entries, "riscv64", None, None);
        assert!(matched.is_empty(), "riscv64 不在支持列表中");
    }

    #[test]
    fn test_filter_os_prefix_kylin10_matches_sp1() {
        let entries = parse_versions();
        // "kylin10" 无精确 aarch64 匹配，但前缀可以命中 kylin10_sp1
        let matched = filter_entries_os_prefix(&entries, "aarch64", None, "kylin10");
        assert!(!matched.is_empty(), "kylin10 前缀应命中 kylin10_sp1");
        assert!(
            matched.iter().all(|e| e.os.starts_with("kylin10")),
            "所有结果应以 kylin10 开头"
        );
    }

    #[test]
    fn test_filter_os_prefix_with_cpu_narrows_to_one() {
        let entries = parse_versions();
        let matched = filter_entries_os_prefix(&entries, "aarch64", Some("kunpeng"), "kylin10");
        assert_eq!(matched.len(), 1, "kunpeng + kylin10 前缀应精确命中 1 条");
        assert_eq!(matched[0].cpu, "kunpeng");
    }

    #[test]
    fn test_filter_os_prefix_sp1_no_match_on_x86() {
        let entries = parse_versions();
        // x86_64 无 kylin10_sp1 条目，前缀 "kylin10_sp1" 不应命中 kylin10_sp3
        let matched = filter_entries_os_prefix(&entries, "x86_64", Some("x86"), "kylin10_sp1");
        assert!(matched.is_empty(), "kylin10_sp1 前缀不应命中 kylin10_sp3");
    }

    #[test]
    fn test_filter_os_prefix_kylin10_base_matches_x86_sp3() {
        let entries = parse_versions();
        // 降级到 "kylin10" 前缀后，x86_64 可命中 kylin10_sp3
        let matched = filter_entries_os_prefix(&entries, "x86_64", Some("x86"), "kylin10");
        assert!(
            !matched.is_empty(),
            "kylin10 前缀应命中 x86_64 上的 kylin10_sp3"
        );
        assert!(matched.iter().all(|e| e.os.starts_with("kylin10")));
    }

    #[test]
    fn test_file_name_extracted_from_url() {
        let entry = VersionEntry {
            arch: "x86_64".into(),
            cpu: "x86".into(),
            os: "rhel7".into(),
            url: "https://example.com/dm8_20260427_x86_rh7_64.zip".into(),
            sha256: None,
        };
        assert_eq!(entry.file_name(), "dm8_20260427_x86_rh7_64.zip");
    }
}
