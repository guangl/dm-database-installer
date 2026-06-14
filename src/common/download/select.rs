use std::collections::HashSet;

use anyhow::{bail, Result};

use super::versions::VersionEntry;

/// 从匹配结果中选出唯一版本。
pub fn select_version<'a>(all: &'a [VersionEntry], matches: &[&'a VersionEntry], arch: &str) -> Result<&'a VersionEntry> {
    match matches.len() {
        1 => Ok(matches[0]),
        0 => bail_no_match(all, arch),
        n => bail_ambiguous(matches, arch, n),
    }
}

fn bail_ambiguous<'a>(matches: &[&'a VersionEntry], arch: &str, n: usize) -> Result<&'a VersionEntry> {
    let candidates: Vec<String> = matches
        .iter()
        .map(|e| format!("  cpu={:<10} os={}", e.cpu, e.os))
        .collect();
    bail!(
        "无法自动识别 CPU/OS 型号（架构 {} 有 {} 个候选）:\n{}\n\n\
         提示: 在 config.toml 设置 installer_package = \"/path/to/dm8.zip\"\
         或 installer_url = \"https://...\" 可跳过自动检测。",
        arch,
        n,
        candidates.join("\n")
    )
}

fn bail_no_match<'a>(all: &[VersionEntry], arch: &str) -> Result<&'a VersionEntry> {
    let for_arch: Vec<String> = all
        .iter()
        .filter(|e| e.arch == arch)
        .map(|e| format!("  cpu={:<10} os={} — {}", e.cpu, e.os, e.file_name()))
        .collect();

    if for_arch.is_empty() {
        let arches: Vec<&str> = {
            let mut seen = HashSet::new();
            all.iter().filter(|e| seen.insert(e.arch.as_str())).map(|e| e.arch.as_str()).collect()
        };
        bail!("不支持当前架构 {}。支持的架构: {}", arch, arches.join(", "));
    }
    bail!(
        "无法自动匹配当前系统 OS。架构 {} 的可用版本:\n{}\n\n\
         提示: 在 config.toml 设置 installer_package = \"/path/to/dm8.zip\"\
         或 installer_url = \"https://...\" 可跳过自动检测。",
        arch,
        for_arch.join("\n")
    )
}
