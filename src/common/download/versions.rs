/// 内嵌 versions.txt，构建时确定，运行时无需文件系统访问。
const VERSIONS_TXT: &str = include_str!("../../../versions.txt");

#[derive(Debug, Clone)]
pub struct VersionEntry {
    pub arch: String,
    pub cpu: String,
    pub os: String,
    pub url: String,
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
    let mut cols = line.splitn(4, '\t');
    let arch = cols.next()?.trim().to_string();
    let cpu  = cols.next()?.trim().to_string();
    let os   = cols.next()?.trim().to_string();
    let url  = cols.next()?.trim().to_string();
    if url.is_empty() { return None; }
    Some(VersionEntry { arch, cpu, os, url })
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
        .filter(|e| cpu.map_or(true, |c| e.cpu == c))
        .filter(|e| os.map_or(true, |o| e.os == o))
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
            assert!(entry.url.starts_with("https://"), "URL 应以 https:// 开头: {}", entry.url);
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
    fn test_file_name_extracted_from_url() {
        let entry = VersionEntry {
            arch: "x86_64".into(),
            cpu: "x86".into(),
            os: "rhel7".into(),
            url: "https://example.com/dm8_20260427_x86_rh7_64.zip".into(),
        };
        assert_eq!(entry.file_name(), "dm8_20260427_x86_rh7_64.zip");
    }
}
