/// 单机安装引导——在用户未提供 --config 时打印。
pub fn print_standalone() {
    eprintln!(
        "\
达梦数据库单机安装 — 引导
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

单机安装需要一个 TOML 配置文件。按以下 3 步操作：

  步骤 1  生成配置模板

            dm-installer init standalone

          默认输出 dm-standalone.toml。如需自定义路径：

            dm-installer init standalone -o /etc/dm/standalone.toml

  步骤 2  编辑配置文件

          用编辑器打开并按需调整（安装路径、端口等）。
          SYSDBA / SYSAUDITOR 密码在安装时由终端提示输入，
          无需写入配置文件。

  步骤 3  执行安装

            dm-installer install --config dm-standalone.toml

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    );
}

/// 集群部署引导——在用户未提供 --config 时打印。
pub fn print_cluster() {
    eprintln!(
        "\
达梦数据库集群部署 — 引导
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

集群部署需要一个 TOML 配置文件（包含节点 IP、SSH
凭证、端口分配等）。请先选择集群类型：

  主备集群（Primary-Standby，推荐入门）
    dm-installer init cluster primary-standby

  读写分离集群（备节点承担只读查询）
    dm-installer init cluster rws

  DSC 共享存储集群（多实例共享 SAN/NFS）
    dm-installer init cluster dsc

  步骤 1  生成并编辑配置文件

          生成后打开配置文件，填写各节点 IP 和 SSH 认证
          信息（私钥路径或密码）。

  步骤 2  执行集群部署

            dm-installer cluster deploy --config <配置文件>

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    );
}
