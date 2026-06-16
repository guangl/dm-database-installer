#!/usr/bin/env bash
# 达梦数据库单机静默安装
# 用法: curl -fsSL https://raw.githubusercontent.com/.../install.sh | bash
set -euo pipefail

# ── 安装参数 ─────────────────────────────────────────────────────────────────────
DM_INSTALL_PATH="/home/dmdba/dmdbms"
DM_DATA_PATH="/home/dmdba/dmdbms/data"
DM_PORT=5236
DM_INSTANCE="DMSERVER"
DM_DB_NAME="DAMENG"
DM_PAGE_SIZE=32
DM_EXTENT_SIZE=32
DM_CHARSET=0
DM_CASE_SENSITIVE=Y
DM_VERSION=""

VERSIONS_URL="https://raw.githubusercontent.com/guangl/dm-database-installer/main/versions.txt"

# ── 颜色输出 ─────────────────────────────────────────────────────────────────────
if [ -t 1 ] && command -v tput >/dev/null 2>&1; then
    RED=$(tput setaf 1); GREEN=$(tput setaf 2)
    YELLOW=$(tput setaf 3); RESET=$(tput sgr0)
else
    RED=""; GREEN=""; YELLOW=""; RESET=""
fi

log_ok()   { printf "%s[OK]%s   %s\n" "$GREEN"  "$RESET" "$*"; }
log_err()  { printf "%s[ERR]%s  %s\n" "$RED"    "$RESET" "$*" >&2; }
log_warn() { printf "%s[WARN]%s %s\n" "$YELLOW" "$RESET" "$*"; }
log_info() { printf "  ·  %s\n" "$*"; }

step_header() { printf "\n%s── %s ──────────────────────────────────────────────%s\n" "$YELLOW" "$*" "$RESET"; }
step_footer()  { printf "%s──────────────────────────────────────────────────────────────%s\n" "$YELLOW" "$RESET"; }

check_ok()   { printf "  %s✓%s  %s\n" "$GREEN"  "$RESET" "$1${2:+: $2}"; }
check_warn() { printf "  %s⚠%s  %s\n" "$YELLOW" "$RESET" "$1${2:+: $2}"; }
check_fail() { printf "  %s✗%s  %s\n" "$RED"    "$RESET" "$1${2:+: $2}" >&2; }

# ── 密码生成 ──────────────────────────────────────────────────────────────────────
# 生成满足达梦密码策略的随机密码（16 位：大写+小写+数字+下划线+12位字母数字）
# 特殊字符仅用 _ 避免 disql 连接串解析歧义（# 会被截断为注释）
generate_password() {
    local u l d body
    u=$(LC_ALL=C tr -dc 'ABCDEFGHJKLMNPQRSTUVWXYZ' < /dev/urandom | head -c 1)
    l=$(LC_ALL=C tr -dc 'abcdefghjkmnpqrstuvwxyz'  < /dev/urandom | head -c 1)
    d=$(LC_ALL=C tr -dc '23456789'                   < /dev/urandom | head -c 1)
    body=$(LC_ALL=C tr -dc 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789' \
           < /dev/urandom | head -c 12)
    printf '%s' "${u}${l}${d}_${body}"
}

# ── 清理与回退 ───────────────────────────────────────────────────────────────────
TMPDIR_WORK=""
_ISO_MOUNT_DIR=""
BACKUP_DIR=""
SUDO=""
_SUDO_KEEPALIVE_PID=""

_DM_SUCCESS=0
_DM_INSTALLED=0
_DM_DB_INITED=0
_DM_SERVER_REGISTERED=0

_restore_file() {
    local src="$1" dst="$2"
    [ -f "$src" ] || return 0
    $SUDO cp "$src" "$dst" 2>/dev/null \
        || log_warn "恢复 $dst 失败，请手动执行: cp $src $dst"
    log_info "已恢复: $dst"
}

_rollback_services() {
    if [ "$_DM_SERVER_REGISTERED" -eq 1 ]; then
        local svc="DmService${DM_INSTANCE}"
        $SUDO systemctl stop    "$svc" 2>/dev/null || true
        $SUDO systemctl disable "$svc" 2>/dev/null || true
        $SUDO rm -f "/etc/systemd/system/${svc}.service" \
              "/usr/lib/systemd/system/${svc}.service" 2>/dev/null || true
        $SUDO systemctl daemon-reload 2>/dev/null || true
        log_info "已回退: ${svc}.service"
    fi
    # dmap 可能在安装阶段即已启动，与服务注册状态无关，始终 kill
    $SUDO pkill -u dmdba dmserver 2>/dev/null || true
    $SUDO pkill -u dmdba dmap     2>/dev/null || true
}

_rollback_dm_files() {
    if [ "$_DM_DB_INITED" -eq 1 ] && [ -n "$DM_DATA_PATH" ]; then
        $SUDO rm -rf "$DM_DATA_PATH"
        log_info "已回退: 数据目录 $DM_DATA_PATH"
    fi
    if [ "$_DM_INSTALLED" -eq 1 ] && [ -n "$DM_INSTALL_PATH" ]; then
        $SUDO rm -rf "$DM_INSTALL_PATH"
        log_info "已回退: 安装目录 $DM_INSTALL_PATH"
    fi
}

_rollback_system_config() {
    [ -n "$BACKUP_DIR" ] && [ -d "$BACKUP_DIR" ] || return 0
    _restore_file "$BACKUP_DIR/sysctl.conf.bak"    /etc/sysctl.conf
    $SUDO sysctl -p >/dev/null 2>&1 || true
    _restore_file "$BACKUP_DIR/limits.conf.bak"    /etc/security/limits.conf
    _restore_file "$BACKUP_DIR/selinux_config.bak" /etc/selinux/config
    _restore_file "$BACKUP_DIR/sshd_config.bak"    /etc/ssh/sshd_config
    $SUDO systemctl reload sshd 2>/dev/null || true
    _restore_file "$BACKUP_DIR/pam_login.bak"      /etc/pam.d/login
    _restore_file "$BACKUP_DIR/rc.local.bak"       /etc/rc.local
    _restore_file "$BACKUP_DIR/profile.bak"        /etc/profile
    if [ -f "$BACKUP_DIR/thp.bak" ]; then
        local thp="/sys/kernel/mm/transparent_hugepage/enabled"
        [ -f "$thp" ] && cat "$BACKUP_DIR/thp.bak" | $SUDO tee "$thp" > /dev/null 2>&1 || true
        log_info "已恢复: THP 原始值"
    fi
    if [ -f "$BACKUP_DIR/firewall.bak" ]; then
        grep -q '^active$' "$BACKUP_DIR/firewall.bak" 2>/dev/null && {
            $SUDO systemctl enable firewalld 2>/dev/null || true
            $SUDO systemctl start  firewalld 2>/dev/null || true
            log_info "已恢复: firewalld"
        } || true
    fi
    if [ -f "$BACKUP_DIR/selinux_mode.bak" ]; then
        case "$(cat "$BACKUP_DIR/selinux_mode.bak")" in
            Enforcing)  $SUDO setenforce 1 2>/dev/null || true
                        log_info "已恢复: SELinux 运行时模式 Enforcing" ;;
            Permissive) $SUDO setenforce 0 2>/dev/null || true ;;
        esac
    fi
    if [ -f "$BACKUP_DIR/timezone.bak" ]; then
        local tz; tz=$(cat "$BACKUP_DIR/timezone.bak")
        [ -n "$tz" ] && $SUDO timedatectl set-timezone "$tz" 2>/dev/null || true
        log_info "已恢复: 时区 $tz"
    fi
}

_rollback_users() {
    if $SUDO userdel -r dmdba 2>/dev/null; then
        log_info "已回退: dmdba 用户"
    fi
    if $SUDO groupdel dinstall 2>/dev/null; then
        log_info "已回退: dinstall 用户组"
    fi
}

rollback() {
    log_warn "安装失败，开始回退..."
    _rollback_services
    _rollback_dm_files
    _rollback_system_config
    _rollback_users
    log_warn "环境回退完成"
}

_on_interrupt() {
    log_warn "安装被用户中断"
    exit 130
}

cleanup() {
    local exit_code=$?         # 必须最先捕获 — trap builtin 本身会重置 $?
    trap '' INT TERM  # 防止回退过程中再次被中断
    [ -n "${_SUDO_KEEPALIVE_PID:-}" ] && kill "$_SUDO_KEEPALIVE_PID" 2>/dev/null || true
    [ "$_DM_SUCCESS" -eq 0 ] && [ "$exit_code" -ne 0 ] && rollback
    [ -n "$_ISO_MOUNT_DIR" ] && $SUDO umount "$_ISO_MOUNT_DIR" 2>/dev/null || true
    [ -n "$TMPDIR_WORK" ] && rm -rf "$TMPDIR_WORK"
    [ -n "$BACKUP_DIR"  ] && rm -rf "$BACKUP_DIR"
}
trap _on_interrupt INT TERM
trap cleanup EXIT

# ── 前置检查 ──────────────────────────────────────────────────────────────────────
check_root() {
    if [ "$(id -u)" -eq 0 ]; then
        SUDO=""
        check_ok "root 权限"
        return 0
    fi
    if ! command -v sudo >/dev/null 2>&1; then
        check_fail "root 权限" "需要 root 权限，请以 root 运行或安装 sudo"
        exit 1
    fi
    log_info "非 root 用户，特权操作将通过 sudo 执行（请在提示时输入密码）..."
    sudo -v || { check_fail "sudo" "密码验证失败"; exit 1; }
    SUDO="sudo"
    check_ok "root 权限" "通过 sudo"
    # 后台保活：防止长时间下载导致 sudo 缓存（默认 15 分钟）在 rollback 时过期
    ( while sudo -n true 2>/dev/null; do sleep 60; done ) &
    _SUDO_KEEPALIVE_PID=$!
}

check_existing_install() {
    if [ -f "$DM_INSTALL_PATH/bin/dmserver" ]; then
        log_warn "已检测到达梦实例，跳过安装"
        exit 0
    fi
}

check_deps() {
    local missing=()
    command -v curl  >/dev/null 2>&1 || missing+=("curl")
    command -v unzip >/dev/null 2>&1 || missing+=("unzip")
    command -v mount >/dev/null 2>&1 || missing+=("mount")
    if [ ${#missing[@]} -gt 0 ]; then
        check_fail "依赖工具" "缺少: ${missing[*]}"
        log_err "Debian/Ubuntu: apt-get install -y ${missing[*]}"
        log_err "RHEL/CentOS:   yum install -y ${missing[*]}"
        exit 1
    fi
    check_ok "依赖工具" "curl unzip mount"
}

check_port() {
    if ss -tlnp 2>/dev/null | grep -q ":${DM_PORT}[^0-9]"; then
        check_fail "端口 ${DM_PORT}" "已被占用，请修改 DM_PORT 或释放该端口"
        exit 1
    fi
    check_ok "端口 ${DM_PORT}" "可用"
}

check_memory() {
    local total_kb
    total_kb=$(grep '^MemTotal:' /proc/meminfo 2>/dev/null | awk '{print $2}')
    local need_kb=$((4 * 1024 * 1024))
    if [ -z "$total_kb" ] || [ "$total_kb" -lt "$need_kb" ]; then
        check_fail "内存" "当前 $((${total_kb:-0} / 1024)) MB，要求 >= 4 GB"
        exit 1
    fi
    check_ok "内存" "$((total_kb / 1024)) MB"
}

check_install_disk() {
    local parent
    parent=$(dirname "$DM_INSTALL_PATH")
    [ -d "$parent" ] || parent="/"
    local avail_kb
    avail_kb=$(df -Pk "$parent" 2>/dev/null | awk 'NR==2{print $4}')
    local need_kb=$((20 * 1024 * 1024))
    if [ -z "$avail_kb" ] || [ "$avail_kb" -lt "$need_kb" ]; then
        check_fail "安装路径磁盘" "可用空间不足，需要 >= 20 GB"
        exit 1
    fi
    check_ok "安装路径磁盘" "$((avail_kb / 1024 / 1024)) GB 可用"
}

check_ulimits() {
    local nofile nproc need=65536 warn=0
    nofile=$(ulimit -n 2>/dev/null)
    nproc=$(ulimit -u 2>/dev/null)

    if [ "$nofile" != "unlimited" ] && [ "${nofile:-0}" -lt "$need" ]; then
        check_warn "ulimit nofile" "当前 ${nofile}，建议 >= ${need}"
        warn=1
    fi
    if [ "$nproc" != "unlimited" ] && [ "${nproc:-0}" -lt "$need" ]; then
        check_warn "ulimit nproc" "当前 ${nproc}，建议 >= ${need}"
        warn=1
    fi

    if [ "$warn" -eq 1 ]; then
        printf "       %s\n" "请在 /etc/security/limits.conf 添加:"
        printf "       %s\n" "  dmdba soft nofile 65536    dmdba hard nofile 65536"
        printf "       %s\n" "  dmdba soft nproc  65536    dmdba hard nproc  65536"
    else
        check_ok "ulimit" "nofile=${nofile} nproc=${nproc}"
    fi
}

check_selinux() {
    if ! command -v getenforce >/dev/null 2>&1; then
        return 0
    fi
    local mode
    mode=$(getenforce 2>/dev/null)
    case "$mode" in
        Enforcing)
            check_warn "SELinux" "Enforcing 模式，可能阻断 DM 进程"
            printf "       %s\n" "临时切换: setenforce 0"
            printf "       %s\n" "永久禁用: 将 /etc/selinux/config SELINUX=enforcing 改为 permissive"
            ;;
        Permissive)
            check_ok "SELinux" "Permissive"
            ;;
        *)
            check_ok "SELinux" "已禁用"
            ;;
    esac
}

# ── 创建 dmdba 系统用户 ───────────────────────────────────────────────────────────
create_dmdba_user() {
    getent group dinstall >/dev/null 2>&1 || {
        $SUDO groupadd -g 1002 dinstall || { log_err "创建用户组 dinstall 失败"; exit 1; }
    }
    if id dmdba >/dev/null 2>&1; then
        log_info "系统用户 dmdba 已存在，跳过创建"
    else
        log_info "创建系统用户 dmdba..."
        $SUDO useradd -u 1002 -g dinstall -m -d /home/dmdba -s /bin/bash dmdba || {
            log_err "创建用户 dmdba 失败"
            exit 1
        }
    fi
    echo 'dmdba:dmdba' | $SUDO chpasswd || {
        log_err "设置 dmdba 密码失败"
        exit 1
    }
    $SUDO chage -M -1 dmdba || log_warn "设置 dmdba 密码永不过期失败，请手动执行: chage -M -1 dmdba"
    # 验证
    id dmdba >/dev/null 2>&1 || { log_err "dmdba 用户创建后验证失败"; exit 1; }
    log_ok "系统用户 dmdba 已就绪"
}

# ── 系统环境配置 ──────────────────────────────────────────────────────────────────
_backup_system_files() {
    local thp="/sys/kernel/mm/transparent_hugepage/enabled"
    local IFS=':'
    local pairs="
        /etc/sysctl.conf:sysctl.conf
        /etc/security/limits.conf:limits.conf
        /etc/selinux/config:selinux_config
        /etc/ssh/sshd_config:sshd_config
        /etc/pam.d/login:pam_login
        /etc/rc.local:rc.local
        /etc/profile:profile"
    while IFS= read -r pair; do
        pair="${pair## }"; [ -z "$pair" ] && continue
        local src="${pair%%:*}" bak="${pair##*:}"
        [ -f "$src" ] && $SUDO cp "$src" "$BACKUP_DIR/${bak}.bak" 2>/dev/null || true
    done <<EOF
$pairs
EOF
    [ -f "$thp" ] && grep -o '\[[a-z]*\]' "$thp" | tr -d '[]' \
        > "$BACKUP_DIR/thp.bak" 2>/dev/null || true
    systemctl is-active firewalld 2>/dev/null \
        > "$BACKUP_DIR/firewall.bak" || echo "inactive" > "$BACKUP_DIR/firewall.bak"
    getenforce 2>/dev/null > "$BACKUP_DIR/selinux_mode.bak" || true
    timedatectl show --property=Timezone --value 2>/dev/null \
        > "$BACKUP_DIR/timezone.bak" || true
}

setup_env() {
    _backup_system_files
    _env_selinux
    _env_thp
    _env_timezone
    _env_locale
    _env_sshd
    _env_firewall
    _env_limits
    _env_pam
    _env_sysctl
}

_env_selinux() {
    $SUDO setenforce 0 2>/dev/null || true
    if [ -f /etc/selinux/config ]; then
        $SUDO sed -i '/^SELINUX=/cSELINUX=disabled' /etc/selinux/config || {
            check_warn "SELinux" "修改 /etc/selinux/config 失败，请手动设置 SELINUX=disabled"
            return
        }
        grep -q '^SELINUX=disabled' /etc/selinux/config \
            || { check_warn "SELinux" "配置写入后验证失败"; return; }
    fi
    if command -v getenforce >/dev/null 2>&1; then
        mode=$(getenforce 2>/dev/null || echo "")
        [ "$mode" = "Enforcing" ] \
            && check_warn "SELinux" "运行时仍为 Enforcing，重启后永久禁用生效" \
            || check_ok   "SELinux" "${mode:-已禁用}"
    else
        check_ok "SELinux" "已禁用"
    fi
}

_env_thp() {
    local thp="/sys/kernel/mm/transparent_hugepage/enabled"
    if [ -f "$thp" ]; then
        echo never | $SUDO tee "$thp" > /dev/null || { check_warn "THP" "关闭失败"; return; }
        grep -q 'never' "$thp" || { check_warn "THP" "验证失败，当前值: $(cat "$thp")"; return; }
    fi
    if [ -f /etc/rc.local ]; then
        grep -q 'transparent_hugepage' /etc/rc.local 2>/dev/null \
            || echo 'echo never > /sys/kernel/mm/transparent_hugepage/enabled' | $SUDO tee -a /etc/rc.local > /dev/null
        $SUDO chmod +x /etc/rc.local
    fi
    check_ok "THP" "已关闭"
}

_env_timezone() {
    $SUDO timedatectl set-timezone Asia/Shanghai 2>/dev/null || {
        check_warn "时区" "设置失败，请手动执行: timedatectl set-timezone Asia/Shanghai"
        return
    }
    timedatectl show --property=Timezone 2>/dev/null | grep -q 'Asia/Shanghai' \
        || check_warn "时区" "验证失败，当前值: $(timedatectl show --property=Timezone 2>/dev/null)"
    check_ok "时区" "Asia/Shanghai"
}

_env_locale() {
    local marker="export LANG=zh_CN.UTF-8"
    grep -q "$marker" /etc/profile 2>/dev/null \
        || echo "$marker" | $SUDO tee -a /etc/profile > /dev/null
    grep -q "$marker" /etc/profile \
        || check_warn "字符集" "/etc/profile 写入验证失败"
    check_ok "字符集" "zh_CN.UTF-8"
}

_env_sshd() {
    [ -f /etc/ssh/sshd_config ] || return 0
    $SUDO sed -i '/^#GSSAPIAuthentication/cGSSAPIAuthentication no' /etc/ssh/sshd_config
    $SUDO sed -i '/^GSSAPIAuthentication/cGSSAPIAuthentication no'  /etc/ssh/sshd_config
    $SUDO sed -i '/^#UseDNS/cUseDNS no'                            /etc/ssh/sshd_config
    $SUDO sed -i '/^UseDNS/cUseDNS no'                             /etc/ssh/sshd_config
    # reload 不断开当前连接，失败时静默忽略（如容器环境无 sshd）
    $SUDO systemctl reload sshd 2>/dev/null || true
    check_ok "SSH" "GSSAPIAuthentication=no  UseDNS=no"
}

_env_firewall() {
    $SUDO systemctl stop firewalld    2>/dev/null || true
    $SUDO systemctl disable firewalld 2>/dev/null || true
    state=$(systemctl is-active firewalld 2>/dev/null || echo "inactive")
    [ "$state" = "active" ] && check_warn "防火墙" "关闭失败，当前状态仍为 active" \
                             || check_ok   "防火墙" "已关闭"
}

_env_limits() {
    local path="/etc/security/limits.conf"
    [ -f "$path" ] || { check_warn "limits.conf" "$path 不存在，跳过配置"; return; }
    local lines=(
        "dmdba  soft  nice     0"   "dmdba  hard  nice     0"
        "dmdba  soft  as       unlimited" "dmdba  hard  as       unlimited"
        "dmdba  soft  fsize    unlimited" "dmdba  hard  fsize    unlimited"
        "dmdba  soft  nproc    65536"     "dmdba  hard  nproc    65536"
        "dmdba  soft  nofile   65536"     "dmdba  hard  nofile   65536"
        "dmdba  soft  core     unlimited" "dmdba  hard  core     unlimited"
        "dmdba  soft  data     unlimited" "dmdba  hard  data     unlimited"
    )
    for line in "${lines[@]}"; do
        grep -qF "$line" "$path" 2>/dev/null || echo "$line" | $SUDO tee -a "$path" > /dev/null
    done
    grep -qF "dmdba  soft  data     unlimited" "$path" \
        || { check_warn "limits.conf" "写入后验证失败"; return; }
    check_ok "limits.conf" "nofile/nproc=65536"
}

_env_pam() {
    local path="/etc/pam.d/login"
    [ -f "$path" ] || return 0
    grep -q 'pam_limits.so' "$path" 2>/dev/null && return 0
    printf '\nsession    required        /lib64/security/pam_limits.so\nsession    required        pam_limits.so\n' | $SUDO tee -a "$path" > /dev/null || {
        log_warn "写入 /etc/pam.d/login 失败"
        return
    }
    grep -q 'pam_limits.so' "$path" || { check_warn "PAM limits" "写入后验证失败"; return; }
    check_ok "PAM limits" "已配置"
}

_env_sysctl() {
    local path="/etc/sysctl.conf"
    local params=(
        "fs.file-max = 6815744"
        "fs.aio-max-nr = 1048576"
        "kernel.shmmni = 4096"
        "kernel.sem = 250 32000 100 128"
        "net.ipv4.ip_local_port_range = 9000 65500"
        "net.core.rmem_default = 4194304"
        "net.core.rmem_max = 4194304"
        "net.core.wmem_default = 262144"
        "net.core.wmem_max = 1048576"
        "vm.swappiness = 0"
        "vm.dirty_background_ratio = 3"
        "vm.dirty_ratio = 80"
        "vm.dirty_expire_centisecs = 500"
        "vm.dirty_writeback_centisecs = 100"
    )
    for param in "${params[@]}"; do
        key="${param%%=*}"; key="${key%% *}"
        grep -q "^${key}" "$path" 2>/dev/null || echo "$param" | $SUDO tee -a "$path" > /dev/null
    done
    if ! grep -q '^kernel.shmall' "$path" 2>/dev/null; then
        mem_kb=$(awk '/^MemTotal:/{print $2}' /proc/meminfo)
        shmall=$(awk -v m="$mem_kb" 'BEGIN{printf "%.0f", m*0.64/4}')
        shmmax=$(awk -v m="$mem_kb" 'BEGIN{printf "%.0f", m*0.64*1024}')
        echo "kernel.shmall=$shmall" | $SUDO tee -a "$path" > /dev/null
        echo "kernel.shmmax=$shmmax" | $SUDO tee -a "$path" > /dev/null
    fi
    $SUDO sysctl -p >/dev/null 2>&1 || { check_warn "sysctl" "sysctl -p 执行失败，内核参数可能未生效"; return; }
    sysctl fs.file-max 2>/dev/null | grep -q '6815744' \
        || check_warn "sysctl" "fs.file-max 验证失败，当前值: $(sysctl fs.file-max 2>/dev/null)"
    check_ok "sysctl" "内核参数已生效"
}

# ── 平台检测（arch / cpu_key / os_key）────────────────────────────────────────────
detect_platform() {
    ARCH=$(uname -m)

    # CPU 型号
    case "$ARCH" in
        x86_64)
            if grep -qi "hygon" /proc/cpuinfo 2>/dev/null; then
                CPU_KEY="hygon"
            else
                CPU_KEY="x86"
            fi
            ;;
        aarch64)
            if grep -qi "phytium\|ft-2000\|ftarm" /proc/cpuinfo 2>/dev/null; then
                CPU_KEY="ft2000"
            else
                CPU_KEY="kunpeng"
            fi
            ;;
        loongarch64) CPU_KEY="ls5000"  ;;
        mips64el)    CPU_KEY="ls4000"  ;;
        sw_64)       CPU_KEY="sw3231"  ;;
        *)
            log_err "不支持的架构: $ARCH"
            exit 1
            ;;
    esac

    # 操作系统
    OS_KEY=""
    if [ -f /etc/os-release ]; then
        os_id=$(grep "^ID=" /etc/os-release | cut -d= -f2 | tr -d '"' | tr '[:upper:]' '[:lower:]')
        os_ver=$(grep "^VERSION_ID=" /etc/os-release | cut -d= -f2 | tr -d '"')
        os_ver_full=$(grep "^VERSION=" /etc/os-release | cut -d= -f2 | tr -d '"')
        case "$os_id" in
            kylin)
                # VERSION_ID="V10" 无 SP 标识；回退到 VERSION 字段的 codename
                # Tercel=SP1, Lance=SP3（实测 opstool/kylin:v10sp1）
                _kylin_str="${os_ver} ${os_ver_full}"
                case "$_kylin_str" in
                    *SP3*|*sp3*|*Lance*) OS_KEY="kylin10_sp3" ;;
                    *SP1*|*sp1*|*Tercel*) OS_KEY="kylin10_sp1" ;;
                    *)                    OS_KEY="kylin10"     ;;
                esac
                unset _kylin_str
                ;;
            uos|uniontech)  OS_KEY="uos20"   ;;
            ubuntu)
                major=$(printf '%s' "$os_ver" | cut -d. -f1)
                OS_KEY="ubuntu${major}"
                ;;
            centos)
                major=$(printf '%s' "$os_ver" | cut -d. -f1)
                [ "$major" -ge 7 ] && OS_KEY="centos7" || OS_KEY="rhel6"
                ;;
            rhel|rocky|almalinux|ol)
                major=$(printf '%s' "$os_ver" | cut -d. -f1)
                [ "$major" -ge 7 ] && OS_KEY="rhel7" || OS_KEY="rhel6"
                ;;
            nfsc|nfs)   OS_KEY="nfsc"   ;;
        esac
    fi
    [ -z "$OS_KEY" ] && OS_KEY="rhel7"   # 兜底：RHEL7 兼容性最广

    log_info "平台检测: arch=${ARCH}  cpu=${CPU_KEY}  os=${OS_KEY}"
}

# ── 从 versions.txt 匹配下载链接 ─────────────────────────────────────────────────
select_download_url() {
    log_info "获取下载链接列表..."
    local versions_data
    versions_data=$(curl -sf --max-time 15 "$VERSIONS_URL") || {
        log_err "无法获取 versions.txt: $VERSIONS_URL"
        exit 1
    }

    # 精确匹配 arch + cpu + os
    DOWNLOAD_URL=$(printf '%s' "$versions_data" \
        | awk -v a="$ARCH" -v c="$CPU_KEY" -v o="$OS_KEY" \
            '$1==a && $2==c && $3==o {print $4; exit}')

    # 回退：同 arch + cpu，os 不限（取第一条）
    if [ -z "$DOWNLOAD_URL" ]; then
        log_warn "未找到 ${CPU_KEY}/${OS_KEY} 精确包，尝试同 CPU 其他 OS..."
        DOWNLOAD_URL=$(printf '%s' "$versions_data" \
            | awk -v a="$ARCH" -v c="$CPU_KEY" \
                '$1==a && $2==c {print $4; exit}')
    fi

    if [ -z "$DOWNLOAD_URL" ]; then
        log_err "versions.txt 中无匹配平台 arch=${ARCH} cpu=${CPU_KEY} os=${OS_KEY}"
        log_err "请到 https://eco.dameng.com/download/ 手动下载"
        exit 1
    fi

    log_ok "匹配安装包: $(basename "$DOWNLOAD_URL")"
}

# ── 选择有足够空间的临时目录（至少 4GB）─────────────────────────────────────────
choose_work_dir() {
    # 达梦安装包 zip ~1.8GB + ISO ~1.5GB，需要约 4GB 可用空间
    local need_kb=$((4 * 1024 * 1024))

    # 优先使用用户指定目录
    if [ -n "${DM_WORKDIR:-}" ]; then
        if [ ! -d "$DM_WORKDIR" ]; then
            log_err "DM_WORKDIR 目录不存在: $DM_WORKDIR"
            exit 1
        fi
        printf '%s' "$DM_WORKDIR"
        return
    fi

    for candidate in /var/tmp /tmp "${HOME:-/root}"; do
        [ -d "$candidate" ] || continue
        local avail_kb
        avail_kb=$(df -Pk "$candidate" 2>/dev/null | awk 'NR==2{print $4}')
        if [ -n "$avail_kb" ] && [ "$avail_kb" -ge "$need_kb" ]; then
            printf '%s' "$candidate"
            return
        fi
    done
    log_err "磁盘空间不足：/var/tmp、/tmp、\$HOME 均低于 4GB 可用"
    log_err "可通过环境变量指定空间充足的目录重试："
    log_err "  DM_WORKDIR=/data/tmp bash install.sh"
    exit 1
}

# ── 下载并解压 ────────────────────────────────────────────────────────────────────
download_and_extract() {
    local base_dir
    base_dir=$(choose_work_dir)
    TMPDIR_WORK=$(mktemp -d -p "$base_dir")
    local zip_file="$TMPDIR_WORK/dm8.zip"
    local extract_dir="$TMPDIR_WORK/dm8_extract"
    mkdir -p "$extract_dir"

    log_info "下载安装包（临时目录: $TMPDIR_WORK）..."
    curl -L -# -o "$zip_file" \
        --max-time 1800 --retry 3 \
        "$DOWNLOAD_URL" || {
        log_err "下载失败: $DOWNLOAD_URL"
        log_err "如网络正常但仍报错，请检查 $base_dir 磁盘可用空间"
        exit 1
    }
    log_ok "下载完成"

    log_info "解压安装包..."
    unzip -q "$zip_file" -d "$extract_dir"

    # zip 内是 ISO，挂载后查找安装文件
    local iso_file
    iso_file=$(find "$extract_dir" -name "*.iso" -type f | head -1)
    if [ -n "$iso_file" ]; then
        log_info "检测到 ISO，挂载中..."
        local iso_dir="$TMPDIR_WORK/dm8_iso"
        mkdir -p "$iso_dir"
        $SUDO mount -o loop,ro "$iso_file" "$iso_dir" || {
            log_err "挂载 ISO 失败"
            exit 1
        }
        _ISO_MOUNT_DIR="$iso_dir"
        extract_dir="$iso_dir"
    fi

    local bin_in_iso
    bin_in_iso=$(find "$extract_dir" -name "DMInstall.bin" -type f | head -1)
    if [ -z "$bin_in_iso" ]; then
        log_err "未在安装包中找到 DMInstall.bin"
        exit 1
    fi
    # ISO 以只读方式挂载，需将文件复制到可写目录后再 chmod
    DM_INSTALL_BIN="$TMPDIR_WORK/DMInstall.bin"
    cp "$bin_in_iso" "$DM_INSTALL_BIN"
    chmod +x "$DM_INSTALL_BIN"
    log_ok "解压完成"
}

# ── 生成响应文件 ──────────────────────────────────────────────────────────────────
write_response_xml() {
    local sys_lang
    case "${LANG:-${LC_ALL:-}}" in
        zh_*) sys_lang="ZH" ;;
        *)    sys_lang="EN" ;;
    esac

    RESPONSE_XML="$TMPDIR_WORK/dm_install.xml"
    cat >"$RESPONSE_XML" <<XML
<?xml version="1.0" encoding="utf-8"?>
<DATABASE>
    <LANGUAGE>${sys_lang}</LANGUAGE>
    <INSTALL_TYPE>0</INSTALL_TYPE>
    <INSTALL_PATH>${DM_INSTALL_PATH}</INSTALL_PATH>
    <INIT_DB>N</INIT_DB>
</DATABASE>
XML
}

# ── 静默安装 ──────────────────────────────────────────────────────────────────────
run_dminstall() {
    log_info "执行静默安装（以下为安装器输出）..."
    # DMInstall.bin 默认使用 /tmp 作为自身临时目录，需要 2GB 可用空间。
    # 将其重定向到我们已验证有足够空间的目录，避免 tmpfs 不足报错。
    if ! DM_INSTALL_TMPDIR="$TMPDIR_WORK" $SUDO "$DM_INSTALL_BIN" -q "$RESPONSE_XML"; then
        log_err "安装失败，请根据上方安装器输出排查原因"
        exit 1
    fi
    _DM_INSTALLED=1
    log_ok "安装完成"
}

# ── 以 dmdba 用户执行 shell 脚本片段 ──────────────────────────────────────────────
# 通过临时脚本文件绕过 su -c 的 shell 展开（密码含 $ 等特殊字符时安全）。
# 调用方式：_dmdba_sh <<EOF ... EOF
# 注意：heredoc 使用非引用定界符，变量在写入文件前由 root 的 bash 展开；
#       展开后的字面值在脚本中用单引号包裹，避免 dmdba shell 二次展开。
_dmdba_sh() {
    local tmp="/home/dmdba/.dminst_$$"
    { printf '#!/bin/sh\n'; cat; } | $SUDO tee "$tmp" > /dev/null
    $SUDO chown dmdba "$tmp"
    $SUDO chmod 600 "$tmp"
    $SUDO su - dmdba -c "sh '$tmp'"
    local rc=$?
    $SUDO rm -f "$tmp"
    return $rc
}

# ── 初始化数据库 ──────────────────────────────────────────────────────────────────
run_dminit() {
    local dminit_bin="$DM_INSTALL_PATH/bin/dminit"
    $SUDO test -x "$dminit_bin" || { log_err "dminit 不存在: $dminit_bin"; exit 1; }

    log_info "以 dmdba 用户初始化数据库实例..."
    if ! _dmdba_sh <<EOF
exec '$dminit_bin' \
    'PATH=$DM_DATA_PATH' \
    'DB_NAME=$DM_DB_NAME' \
    'INSTANCE_NAME=$DM_INSTANCE' \
    'PORT_NUM=$DM_PORT' \
    'PAGE_SIZE=$DM_PAGE_SIZE' \
    'EXTENT_SIZE=$DM_EXTENT_SIZE' \
    'CASE_SENSITIVE=$DM_CASE_SENSITIVE' \
    'CHARSET=$DM_CHARSET' \
    'SYSDBA_PWD=$SYSDBA_PWD' \
    'SYSAUDITOR_PWD=$SYSAUDITOR_PWD'
EOF
    then
        log_err "数据库初始化失败，请根据上方 dminit 输出排查原因"
        exit 1
    fi
    _DM_DB_INITED=1
    log_ok "数据库初始化完成"
}

# ── 注册 systemd 服务 ────────────────────────────────────────────────────────────
register_service() {
    local service_script="$DM_INSTALL_PATH/script/root/dm_service_installer.sh"
    if ! $SUDO test -f "$service_script"; then
        log_warn "未找到 dm_service_installer.sh，跳过服务注册"
        return 0
    fi
    $SUDO chmod +x "$service_script"

    # 注册并启动 dmserver 数据库服务
    log_info "注册 dmserver 数据库服务..."
    local dm_ini="$DM_DATA_PATH/$DM_DB_NAME/dm.ini"
    $SUDO bash "$service_script" -t dmserver \
        -p "$DM_INSTANCE" \
        -dm_ini "$dm_ini" || {
        log_err "dmserver 服务注册失败"
        exit 1
    }
    _DM_SERVER_REGISTERED=1

    local service_name="DmService${DM_INSTANCE}"
    local service_bin="$DM_INSTALL_PATH/bin/${service_name}"
    log_info "以 dmdba 用户启动数据库服务..."
    $SUDO su - dmdba -c "${service_bin} start" || {
        log_err "数据库服务启动失败，请检查: $DM_DATA_PATH/$DM_DB_NAME/dm_${DM_INSTANCE}.log"
        exit 1
    }
    log_info "等待数据库就绪..."
    _dm_wait_ready
    query_dm_version
    log_ok "服务注册完成: ${service_name}"
}

# ── 等待数据库就绪 ────────────────────────────────────────────────────────────────
_dm_wait_ready() {
    local disql_bin="$DM_INSTALL_PATH/bin/disql"
    local log_file="$DM_DATA_PATH/$DM_DB_NAME/dm_${DM_INSTANCE}.log"
    local attempt=1
    while [ "$attempt" -le 60 ]; do
        if ! pgrep -u dmdba dmserver >/dev/null 2>&1; then
            log_err "dmserver 进程已退出，请检查日志: $log_file"
            exit 1
        fi
        if _dmdba_sh <<EOF >/dev/null 2>&1
printf 'exit;\n' | '$disql_bin' 'SYSDBA/${SYSDBA_PWD}@localhost:${DM_PORT}'
EOF
        then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 2
    done
    log_err "数据库未在 120 秒内就绪，请检查日志: $log_file"
    exit 1
}

# ── 查询数据库版本 ────────────────────────────────────────────────────────────────
query_dm_version() {
    local disql_bin="$DM_INSTALL_PATH/bin/disql"
    local sql_tmp out_tmp
    sql_tmp=$(mktemp)
    out_tmp=$(mktemp)
    printf 'SELECT id_code;\nexit;\n' > "$sql_tmp"
    chmod 644 "$sql_tmp"
    $SUDO chmod 666 "$out_tmp"

    # 让 dmdba 脚本直接把 disql 输出写入 out_tmp，避免依赖 fd 跨 sudo 继承
    _dmdba_sh 2>/dev/null <<EOF
'$disql_bin' 'SYSDBA/${SYSDBA_PWD}@localhost:${DM_PORT}' < '$sql_tmp' > '$out_tmp' 2>/dev/null
EOF

    DM_VERSION=$(awk 'f && NF {print $NF; exit} /^-+/{f=1}' "$out_tmp")
    rm -f "$sql_tmp" "$out_tmp"
}

# ── 完成提示 ──────────────────────────────────────────────────────────────────────
print_success() {
    local charset_name
    case "$DM_CHARSET" in
        0) charset_name="GB18030" ;;
        1) charset_name="UTF-8" ;;
        2) charset_name="EUC-KR" ;;
        *) charset_name="$DM_CHARSET" ;;
    esac
    cat <<EOF

${GREEN}✓ 达梦数据库安装完成${RESET}

  安装路径    : $DM_INSTALL_PATH
  数据路径    : $DM_DATA_PATH/$DM_DB_NAME
  监听端口    : $DM_PORT

╔═══════════════════════════════════════════════════╗
║            达梦数据库初始化参数                   ║
╠═══════════════════════════════════════════════════╣
║  数据库版本: $(printf '%-37s' "${DM_VERSION:-未知}")║
║  数据库名  : $(printf '%-37s' "$DM_DB_NAME")║
║  实例名    : $(printf '%-37s' "$DM_INSTANCE")║
║  页大小    : $(printf '%-37s' "${DM_PAGE_SIZE} KB")║
║  簇大小    : $(printf '%-37s' "$DM_EXTENT_SIZE")║
║  字符集    : $(printf '%-37s' "$charset_name")║
║  大小写敏感: $(printf '%-37s' "$DM_CASE_SENSITIVE")║
╠═══════════════════════════════════════════════════╣
║  SYSDBA     密码: $(printf '%-32s' "$SYSDBA_PWD")║
║  SYSAUDITOR 密码: $(printf '%-32s' "$SYSAUDITOR_PWD")║
╠═══════════════════════════════════════════════════╣
║  首次登录后请立即修改密码                         ║
╚═══════════════════════════════════════════════════╝

  连接测试  : $DM_INSTALL_PATH/bin/disql SYSDBA/${SYSDBA_PWD}@localhost:${DM_PORT}
  查看状态  : systemctl status DmService${DM_INSTANCE}.service

EOF
}

# ── 主流程 ───────────────────────────────────────────────────────────────────────
main() {
    printf "%s\n" "${YELLOW}╔══════════════════════════════════════════════════════════════╗${RESET}"
    printf "%s\n" "${YELLOW}║${RED}  ⚠  仅限开发 / 测试环境使用，严禁用于生产环境！            ${YELLOW}║${RESET}"
    printf "%s\n" "${YELLOW}║     此脚本会修改内核参数、关闭 SELinux 和防火墙。            ║${RESET}"
    printf "%s\n" "${YELLOW}╚══════════════════════════════════════════════════════════════╝${RESET}"
    printf "\n"

    step_header "[1/6] 环境预检"
    check_root
    check_existing_install
    check_deps
    check_port
    check_memory
    check_install_disk
    check_ulimits
    check_selinux
    step_footer

    BACKUP_DIR=$(mktemp -d)

    step_header "[2/6] 系统准备"
    create_dmdba_user
    setup_env
    SYSDBA_PWD=$(generate_password)
    SYSAUDITOR_PWD=$(generate_password)
    step_footer

    step_header "[3/6] 下载安装包"
    detect_platform
    select_download_url
    download_and_extract
    step_footer

    step_header "[4/6] 静默安装"
    write_response_xml
    run_dminstall
    step_footer

    step_header "[5/6] 初始化数据库"
    run_dminit
    step_footer

    step_header "[6/6] 注册服务"
    register_service
    step_footer

    print_success
    _DM_SUCCESS=1
}

main
