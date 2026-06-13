use std::collections::HashSet;

use anyhow::{bail, Result};

use super::versions::VersionEntry;

/// 从匹配结果中选出唯一版本。多个匹配是数据错误，零个匹配是平台不支持。
pub fn select_version<'a>(all: &'a [VersionEntry], matches: &[&'a VersionEntry], arch: &str) -> Result<&'a VersionEntry> {
    match matches.len() {
        1 => Ok(matches[0]),
        0 => bail_no_match(all, arch),
        n => bail!("内部错误: 平台 {} 匹配到 {} 个版本，versions.txt 存在重复条目", arch, n),
    }
}

fn bail_no_match<'a>(all: &[VersionEntry], arch: &str) -> Result<&'a VersionEntry> {
    let for_arch: Vec<String> = all
        .iter()
        .filter(|e| e.arch == arch)
        .map(|e| format!("  {} {} - {}", e.cpu, e.os, e.file_name()))
        .collect();

    if for_arch.is_empty() {
        let arches: Vec<&str> = {
            let mut seen = HashSet::new();
            all.iter().filter(|e| seen.insert(e.arch.as_str())).map(|e| e.arch.as_str()).collect()
        };
        bail!("不支持当前架构 {}。支持的架构: {}", arch, arches.join(", "));
    }
    bail!("无法自动匹配当前系统 OS。架构 {} 的可用版本:\n{}", arch, for_arch.join("\n"));
}
