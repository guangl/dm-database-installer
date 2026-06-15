use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// 对给定文件计算 SHA-256 并与期望值比较。
/// expected_hex 大小写不敏感（统一转换为小写后比较）。
pub fn verify_sha256(path: &Path, expected_hex: &str) -> Result<()> {
    let actual = compute_sha256(path)?;
    let expected_lower = expected_hex.to_lowercase();
    if actual != expected_lower {
        anyhow::bail!(
            "SHA-256 校验失败\n  期望: {}\n  实际: {}",
            expected_lower,
            actual
        );
    }
    Ok(())
}

/// 计算文件的 SHA-256 hex 字符串（内部辅助函数，保持 verify_sha256 < 40 行）。
fn compute_sha256(path: &Path) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("无法打开文件: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hash_bytes = hasher.finalize();
    Ok(hash_bytes.iter().map(|b| format!("{:02x}", b)).collect())
}

use anyhow::Context;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // "hello" 的 SHA-256（小写 hex）
    const HELLO_SHA256: &str =
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    #[test]
    fn test_correct_checksum_passes() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello").unwrap();
        assert!(
            verify_sha256(file.path(), HELLO_SHA256).is_ok(),
            "正确 checksum 应返回 Ok"
        );
    }

    #[test]
    fn test_wrong_checksum_fails() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello").unwrap();
        let wrong_hex = "0000000000000000000000000000000000000000000000000000000000000000";
        let err = verify_sha256(file.path(), wrong_hex).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("SHA-256 校验失败"),
            "错误消息应含 'SHA-256 校验失败'，实际: {msg}"
        );
        assert!(msg.contains("期望"), "错误消息应含 '期望'，实际: {msg}");
        assert!(msg.contains("实际"), "错误消息应含 '实际'，实际: {msg}");
    }

    #[test]
    fn test_uppercase_checksum_accepted() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello").unwrap();
        let upper_hex = HELLO_SHA256.to_uppercase();
        assert!(
            verify_sha256(file.path(), &upper_hex).is_ok(),
            "大写 hex checksum 应通过"
        );
    }
}
