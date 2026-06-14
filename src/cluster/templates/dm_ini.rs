/// 生成 dm.ini 集群追加片段（在现有单机 dm.ini 参数基础上追加）。
///
/// 追加字段说明：
/// - MAL_INI = 1：启用 MAL 系统（多活链路）
/// - ARCH_INI = 1：启用归档
/// - ALTER_MODE_STATUS = 0：初始值 0，SQL 设置主备角色时临时改为 1
/// - ENABLE_OFFLINE_TS = 2：集群模式推荐值
pub fn generate_dm_ini_cluster_suffix() -> String {
    "MAL_INI = 1\nARCH_INI = 1\nALTER_MODE_STATUS = 0\nENABLE_OFFLINE_TS = 2\n".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dm_ini_cluster_suffix_contains_required_fields() {
        let suffix = generate_dm_ini_cluster_suffix();
        assert!(suffix.contains("MAL_INI = 1"), "缺少 MAL_INI = 1");
        assert!(suffix.contains("ARCH_INI = 1"), "缺少 ARCH_INI = 1");
        assert!(suffix.contains("ALTER_MODE_STATUS = 0"), "缺少 ALTER_MODE_STATUS = 0");
        assert!(suffix.contains("ENABLE_OFFLINE_TS = 2"), "缺少 ENABLE_OFFLINE_TS = 2");
    }
}
