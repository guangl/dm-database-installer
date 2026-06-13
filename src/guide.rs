/// 在当前目录未找到合法 config.toml 时打印安装引导。
pub fn print_install() {
    eprintln!(
        "\
达梦数据库安装 — 引导
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

未找到当前目录的 config.toml，请先生成配置模板：

  单机安装（开发 / 测试环境）
    dm-installer init standalone

  主备集群（推荐生产入门）
    dm-installer init cluster primary-standby

  读写分离集群
    dm-installer init cluster rws

  DSC 共享存储集群
    dm-installer init cluster dsc

生成后编辑配置文件，然后执行安装：

    dm-installer install

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    );
}
