use serde::Deserialize;

pub mod validate;

/// 安装配置。Phase 1 以硬编码默认值构造；Phase 2 从 TOML 文件反序列化。
#[derive(Debug, Deserialize)]
pub struct InstallConfig {
    /// DM 安装根目录，默认 /opt/dmdbms
    #[serde(default = "default_install_path")]
    pub install_path: String,

    /// 数据文件目录，默认 /opt/dmdbms/data
    #[serde(default = "default_data_path")]
    pub data_path: String,

    /// 数据库实例名，默认 DMSERVER
    #[serde(default = "default_instance_name")]
    pub instance_name: String,

    /// 监听端口，默认 5236
    #[serde(default = "default_port")]
    pub port: u16,

    /// 页大小（KB），可选 4/8/16/32，默认 8
    #[serde(default = "default_page_size")]
    pub page_size: u8,

    /// 字符集：0=GB18030, 1=UTF-8, 2=EUC-KR，默认 0
    #[serde(default = "default_charset")]
    pub charset: u8,

    /// 大小写敏感，默认 true
    #[serde(default = "default_case_sensitive")]
    pub case_sensitive: bool,

    /// 区段大小（页数），可选 16/32，默认 16
    #[serde(default = "default_extent_size")]
    pub extent_size: u8,
}

fn default_install_path() -> String { "/opt/dmdbms".to_string() }
fn default_data_path() -> String { "/opt/dmdbms/data".to_string() }
fn default_instance_name() -> String { "DMSERVER".to_string() }
fn default_port() -> u16 { 5236 }
fn default_page_size() -> u8 { 8 }
fn default_charset() -> u8 { 0 }
fn default_case_sensitive() -> bool { true }
fn default_extent_size() -> u8 { 16 }

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            install_path: default_install_path(),
            data_path: default_data_path(),
            instance_name: default_instance_name(),
            port: default_port(),
            page_size: default_page_size(),
            charset: default_charset(),
            case_sensitive: default_case_sensitive(),
            extent_size: default_extent_size(),
        }
    }
}
