use std::collections::HashSet;
use std::io::{BufRead, Write};

use anyhow::{bail, Result};

use super::versions::VersionEntry;

/// 根据平台匹配结果选择版本，`non_interactive` 时自动取第一项。
pub fn select_version<'a>(
    all: &'a [VersionEntry],
    matches: &[&'a VersionEntry],
    arch: &str,
    non_interactive: bool,
) -> Result<&'a VersionEntry> {
    if matches.is_empty() {
        return bail_no_match(all, arch);
    }
    if matches.len() == 1 || non_interactive {
        let entry = matches[0];
        if matches.len() == 1 {
            println!("匹配版本: {} ({} {})", entry.file_name(), entry.cpu, entry.os);
        } else {
            println!("多个匹配，自动选择: {} ({} {})", entry.file_name(), entry.cpu, entry.os);
        }
        return Ok(entry);
    }
    prompt_selection(matches)
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

fn prompt_selection<'a>(matches: &[&'a VersionEntry]) -> Result<&'a VersionEntry> {
    println!("检测到多个匹配版本，请选择：");
    for (i, e) in matches.iter().enumerate() {
        println!("  [{}] {} ({} {})", i + 1, e.file_name(), e.cpu, e.os);
    }
    print!("请输入编号 [1-{}]: ", matches.len());
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().lock().read_line(&mut input)?;
    let n: usize = input
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("无效输入: {}", input.trim()))?;

    if n == 0 || n > matches.len() {
        bail!("编号 {} 超出范围 [1-{}]", n, matches.len());
    }
    Ok(matches[n - 1])
}
