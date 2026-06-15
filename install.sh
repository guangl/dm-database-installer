#!/usr/bin/env bash
# 达梦数据库单机静默安装
# 用法: curl -fsSL https://raw.githubusercontent.com/.../install.sh | bash
set -euo pipefail

# ── 安装参数 ─────────────────────────────────────────────────────────────────────
DM_INSTALL_PATH="/home/dmdba/dmdbms"
DM_DATA_PATH="/home/dmdba/dmdbms/data/DAMENG"
DM_PORT=5236
DM_INSTANCE="DMSERVER"
DM_DB_NAME="DAMENG"
DM_PAGE_SIZE=32
DM_EXTENT_SIZE=32
DM_CHARSET=0
DM_CASE_SENSITIVE=Y
DM_ARCH_PATH="$DM_DATA_PATH/arch"
DM_ARCH_FILE_SIZE=128
DM_ARCH_SPACE_LIMIT=0

# GitHub 版本用此 URL；Gitee 发布时 CI/CD 替换为：
# https://raw.giteeusercontent.com/guangluo/dm-database-installer/raw/main/versions.txt
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
log_info() { printf "[--]   %s\n" "$*"; }

# ── 密码生成 ──────────────────────────────────────────────────────────────────────
# 生成满足达梦密码策略的随机密码（16 位，含大写/小写/数字/特殊字符）
generate_password() {
    local u l d s body
    u=$(LC_ALL=C tr -dc 'ABCDEFGHJKLMNPQRSTUVWXYZ' < /dev/urandom | head -c 1)
    l=$(LC_ALL=C tr -dc 'abcdefghjkmnpqrstuvwxyz'  < /dev/urandom | head -c 1)
    d=$(LC_ALL=C tr -dc '23456789'                   < /dev/urandom | head -c 1)
    s=$(LC_ALL=C tr -dc '@#$%&*!'                    < /dev/urandom | head -c 1)
    body=$(LC_ALL=C tr -dc 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789@#$%&*!' \
           < /dev/urandom | head -c 12)
    printf '%s' "${u}${l}${d}${s}${body}" \
        | fold -w1 \
        | awk 'BEGIN{srand()} {print rand() " " $0}' \
        | sort -k1 -n \
        | awk '{printf $2}'
}

# ── 清理 ─────────────────────────────────────────────────────────────────────────
TMPDIR_WORK=""
cleanup() { [ -n "$TMPDIR_WORK" ] && rm -rf "$TMPDIR_WORK"; }
trap cleanup EXIT

# ── 前置检查 ──────────────────────────────────────────────────────────────────────
check_root() {
    if [ "$(id -u)" -ne 0 ]; then
        log_err "需要 root 权限，请以 root 或 sudo 运行"
        exit 1
    fi
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
        log_err "缺少必要工具: ${missing[*]}"
        log_err "Debian/Ubuntu: apt-get install -y ${missing[*]}"
        log_err "RHEL/CentOS:   yum install -y ${missing[*]}"
        exit 1
    fi
}

check_port() {
    if ss -tlnp 2>/dev/null | grep -q ":${DM_PORT}[^0-9]"; then
        log_err "端口 ${DM_PORT} 已被占用，请修改 DM_PORT 或释放该端口"
        exit 1
    fi
    log_info "端口 ${DM_PORT} 可用"
}

check_memory() {
    local total_kb
    total_kb=$(grep '^MemTotal:' /proc/meminfo 2>/dev/null | awk '{print $2}')
    local need_kb=$((4 * 1024 * 1024))
    if [ -z "$total_kb" ] || [ "$total_kb" -lt "$need_kb" ]; then
        log_err "内存不足：当前 $((${total_kb:-0} / 1024)) MB，要求 >= 4 GB"
        exit 1
    fi
    log_info "内存检查通过：$((total_kb / 1024)) MB"
}

check_install_disk() {
    local parent
    parent=$(dirname "$DM_INSTALL_PATH")
    [ -d "$parent" ] || parent="/"
    local avail_kb
    avail_kb=$(df -Pk "$parent" 2>/dev/null | awk 'NR==2{print $4}')
    local need_kb=$((20 * 1024 * 1024))
    if [ -z "$avail_kb" ] || [ "$avail_kb" -lt "$need_kb" ]; then
        log_err "安装路径 ${DM_INSTALL_PATH} 所在分区可用空间不足，需要 >= 20 GB"
        exit 1
    fi
    log_info "安装路径磁盘检查通过：$((avail_kb / 1024 / 1024)) GB 可用"
}

check_ulimits() {
    local nofile nproc need=65536 warn=0
    nofile=$(ulimit -n 2>/dev/null)
    nproc=$(ulimit -u 2>/dev/null)

    if [ "$nofile" != "unlimited" ] && [ "${nofile:-0}" -lt "$need" ]; then
        log_warn "open files (nofile) 当前值 ${nofile}，建议 >= ${need}"
        warn=1
    fi
    if [ "$nproc" != "unlimited" ] && [ "${nproc:-0}" -lt "$need" ]; then
        log_warn "max user processes (nproc) 当前值 ${nproc}，建议 >= ${need}"
        warn=1
    fi

    if [ "$warn" -eq 1 ]; then
        log_warn "ulimit 偏低，DM 运行时可能遇到资源耗尽；请在 /etc/security/limits.conf 添加："
        log_warn "  dmdba soft nofile 65536    dmdba hard nofile 65536"
        log_warn "  dmdba soft nproc  65536    dmdba hard nproc  65536"
    else
        log_info "ulimit 检查通过: nofile=${nofile} nproc=${nproc}"
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
            log_warn "SELinux 处于 Enforcing 模式，可能阻断 DM 进程启动"
            log_warn "临时切换: setenforce 0"
            log_warn "永久禁用: 将 /etc/selinux/config 中 SELINUX=enforcing 改为 permissive，然后重启"
            ;;
        Permissive)
            log_info "SELinux 模式: Permissive，不影响安装"
            ;;
        *)
            log_info "SELinux 已禁用"
            ;;
    esac
}

# ── 创建 dmdba 系统用户 ───────────────────────────────────────────────────────────
create_dmdba_user() {
    getent group dinstall >/dev/null 2>&1 || groupadd -g 1002 dinstall || {
        log_err "创建用户组 dinstall 失败"
        exit 1
    }
    if id dmdba >/dev/null 2>&1; then
        log_info "系统用户 dmdba 已存在，跳过创建"
    else
        log_info "创建系统用户 dmdba..."
        useradd -u 1002 -g dinstall -m -d /home/dmdba -s /bin/bash dmdba || {
            log_err "创建用户 dmdba 失败"
            exit 1
        }
    fi
    echo 'dmdba:dmdba' | chpasswd || {
        log_err "设置 dmdba 密码失败"
        exit 1
    }
    chage -M -1 dmdba || log_warn "设置 dmdba 密码永不过期失败，请手动执行: chage -M -1 dmdba"
    # 验证
    id dmdba >/dev/null 2>&1 || { log_err "dmdba 用户创建后验证失败"; exit 1; }
    log_ok "系统用户 dmdba 已就绪"
}

# ── 系统环境配置 ──────────────────────────────────────────────────────────────────
setup_env() {
    log_info "配置系统环境参数..."
    _env_selinux
    _env_thp
    _env_timezone
    _env_locale
    _env_sshd
    _env_firewall
    _env_limits
    _env_pam
    _env_sysctl
    log_ok "系统环境参数配置完成"
}

_env_selinux() {
    setenforce 0 2>/dev/null || true
    if [ -f /etc/selinux/config ]; then
        sed -i '/^SELINUX=/cSELINUX=disabled' /etc/selinux/config || {
            log_warn "修改 /etc/selinux/config 失败，请手动设置 SELINUX=disabled"
            return
        }
        grep -q '^SELINUX=disabled' /etc/selinux/config \
            || { log_warn "SELinux 配置写入后验证失败"; return; }
    fi
    if command -v getenforce >/dev/null 2>&1; then
        mode=$(getenforce 2>/dev/null || echo "")
        [ "$mode" = "Enforcing" ] \
            && log_warn "SELinux 运行时仍为 Enforcing，重启后永久禁用生效" \
            || log_info "SELinux 状态: ${mode:-已禁用}"
    fi
}

_env_thp() {
    local thp="/sys/kernel/mm/transparent_hugepage/enabled"
    if [ -f "$thp" ]; then
        echo never > "$thp" || { log_warn "关闭 THP 失败"; return; }
        grep -q 'never' "$thp" || { log_warn "THP 关闭后验证失败，当前值: $(cat "$thp")"; return; }
    fi
    if [ -f /etc/rc.local ]; then
        grep -q 'transparent_hugepage' /etc/rc.local 2>/dev/null \
            || echo 'echo never > /sys/kernel/mm/transparent_hugepage/enabled' >> /etc/rc.local
        chmod +x /etc/rc.local
    fi
    log_info "Transparent Hugepages 已关闭"
}

_env_timezone() {
    timedatectl set-timezone Asia/Shanghai 2>/dev/null || {
        log_warn "设置时区失败，请手动执行: timedatectl set-timezone Asia/Shanghai"
        return
    }
    timedatectl show --property=Timezone 2>/dev/null | grep -q 'Asia/Shanghai' \
        || log_warn "时区设置后验证失败，当前值: $(timedatectl show --property=Timezone 2>/dev/null)"
    log_info "时区已设置为 Asia/Shanghai"
}

_env_locale() {
    local marker="export LANG=zh_CN.UTF-8"
    grep -q "$marker" /etc/profile 2>/dev/null \
        || echo "$marker" >> /etc/profile
    grep -q "$marker" /etc/profile \
        || log_warn "/etc/profile 中 LANG 写入验证失败"
    log_info "字符集已设置为 zh_CN.UTF-8"
}

_env_sshd() {
    [ -f /etc/ssh/sshd_config ] || return 0
    sed -i '/^#GSSAPIAuthentication/cGSSAPIAuthentication no' /etc/ssh/sshd_config
    sed -i '/^GSSAPIAuthentication/cGSSAPIAuthentication no'  /etc/ssh/sshd_config
    sed -i '/^#UseDNS/cUseDNS no'                            /etc/ssh/sshd_config
    sed -i '/^UseDNS/cUseDNS no'                             /etc/ssh/sshd_config
    # reload 不断开当前连接，失败时静默忽略（如容器环境无 sshd）
    systemctl reload sshd 2>/dev/null || true
    log_info "SSH 配置已优化（GSSAPIAuthentication=no, UseDNS=no）"
}

_env_firewall() {
    systemctl stop firewalld    2>/dev/null || true
    systemctl disable firewalld 2>/dev/null || true
    state=$(systemctl is-active firewalld 2>/dev/null || echo "inactive")
    [ "$state" = "active" ] && log_warn "防火墙关闭失败，当前状态仍为 active" \
                             || log_info "防火墙已关闭"
}

_env_limits() {
    local path="/etc/security/limits.conf"
    [ -f "$path" ] || { log_warn "$path 不存在，跳过 limits 配置"; return; }
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
        grep -qF "$line" "$path" 2>/dev/null || echo "$line" >> "$path"
    done
    # 抽查最后一条是否写入
    grep -qF "dmdba  soft  data     unlimited" "$path" \
        || log_warn "limits.conf 写入后验证失败"
    log_info "limits.conf 已配置"
}

_env_pam() {
    local path="/etc/pam.d/login"
    [ -f "$path" ] || return 0
    grep -q 'pam_limits.so' "$path" 2>/dev/null && return 0
    printf '\nsession    required        /lib64/security/pam_limits.so\nsession    required        pam_limits.so\n' >> "$path" || {
        log_warn "写入 /etc/pam.d/login 失败"
        return
    }
    grep -q 'pam_limits.so' "$path" || log_warn "/etc/pam.d/login 写入后验证失败"
    log_info "PAM limits 已配置"
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
        grep -q "^${key}" "$path" 2>/dev/null || echo "$param" >> "$path"
    done
    if ! grep -q '^kernel.shmall' "$path" 2>/dev/null; then
        mem_kb=$(awk '/^MemTotal:/{print $2}' /proc/meminfo)
        shmall=$(awk -v m="$mem_kb" 'BEGIN{printf "%.0f", m*0.64/4}')
        shmmax=$(awk -v m="$mem_kb" 'BEGIN{printf "%.0f", m*0.64*1024}')
        echo "kernel.shmall=$shmall" >> "$path"
        echo "kernel.shmmax=$shmmax" >> "$path"
    fi
    sysctl -p >/dev/null 2>&1 || { log_warn "sysctl -p 执行失败，内核参数可能未生效"; return; }
    # 验证关键参数
    sysctl fs.file-max 2>/dev/null | grep -q '6815744' \
        || log_warn "sysctl fs.file-max 验证失败，当前值: $(sysctl fs.file-max 2>/dev/null)"
    log_info "sysctl 内核参数已生效"
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
        mount -o loop,ro "$iso_file" "$iso_dir" || {
            log_err "挂载 ISO 失败，请确认以 root 运行"
            exit 1
        }
        trap 'umount "$TMPDIR_WORK/dm8_iso" 2>/dev/null; [ -n "$TMPDIR_WORK" ] && rm -rf "$TMPDIR_WORK"' EXIT
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
    if ! DM_INSTALL_TMPDIR="$TMPDIR_WORK" "$DM_INSTALL_BIN" -q "$RESPONSE_XML"; then
        log_err "安装失败，请根据上方安装器输出排查原因"
        exit 1
    fi
    log_ok "安装完成"
}

# ── 初始化数据库 ──────────────────────────────────────────────────────────────────
run_dminit() {
    local dminit_bin="$DM_INSTALL_PATH/bin/dminit"
    [ -x "$dminit_bin" ] || { log_err "dminit 不存在: $dminit_bin"; exit 1; }

    log_info "初始化数据库实例（以下为 dminit 输出）..."
    if ! "$dminit_bin" \
        "PATH=$DM_DATA_PATH" \
        "DB_NAME=$DM_DB_NAME" \
        "INSTANCE_NAME=$DM_INSTANCE" \
        "PORT_NUM=$DM_PORT" \
        "PAGE_SIZE=$DM_PAGE_SIZE" \
        "EXTENT_SIZE=$DM_EXTENT_SIZE" \
        "CASE_SENSITIVE=$DM_CASE_SENSITIVE" \
        "CHARSET=$DM_CHARSET" \
        "SYSDBA_PWD=$SYSDBA_PWD" \
        "SYSAUDITOR_PWD=$SYSAUDITOR_PWD"; then
        log_err "数据库初始化失败，请根据上方 dminit 输出排查原因"
        exit 1
    fi
    log_ok "数据库初始化完成"
}

# ── 写入 dmarch.ini ────────────────────────────────────────────────────────────
write_dmarch_ini() {
    log_info "写入本地归档配置 dmarch.ini..."
    mkdir -p "$DM_ARCH_PATH"
    cat > "$DM_DATA_PATH/dmarch.ini" <<EOF
[ARCHIVE_LOCAL1]
ARCH_TYPE = LOCAL
ARCH_DEST = $DM_ARCH_PATH
ARCH_FILE_SIZE = $DM_ARCH_FILE_SIZE
ARCH_SPACE_LIMIT = $DM_ARCH_SPACE_LIMIT
ARCH_HANG_FLAG = 0
ARCH_COMPRESSED = 0
EOF
    log_ok "dmarch.ini 写入完成: $DM_DATA_PATH/dmarch.ini"
}

# ── 注册 systemd 服务 ────────────────────────────────────────────────────────────
register_service() {
    local service_script="$DM_INSTALL_PATH/script/root/dm_service_installer.sh"
    if [ ! -f "$service_script" ]; then
        log_warn "未找到 dm_service_installer.sh，跳过服务注册"
        return 0
    fi
    chmod +x "$service_script"

    # 1. 注册并启动 DMAP 辅助进程服务
    log_info "注册 DMAP 辅助进程服务..."
    bash "$service_script" -t dmap || {
        log_err "DMAP 服务注册失败"
        exit 1
    }
    systemctl enable DmAPService \
        || log_warn "请手动执行: systemctl enable DmAPService"
    systemctl start DmAPService \
        || log_warn "DmAPService 启动失败，请检查: journalctl -u DmAPService"

    # 2. 注册并启动 dmserver 数据库服务
    log_info "注册 dmserver 数据库服务..."
    local dm_ini="$DM_DATA_PATH/$DM_DB_NAME/dm.ini"
    bash "$service_script" -t dmserver \
        -p "$dm_ini" \
        -m auto || {
        log_err "dmserver 服务注册失败"
        exit 1
    }

    local service_name="DmService${DM_INSTANCE}"
    systemctl enable "${service_name}.service" \
        || log_warn "请手动执行: systemctl enable ${service_name}.service"
    systemctl start "${service_name}.service" \
        || log_warn "服务启动失败，请检查: journalctl -u ${service_name}.service"
    log_ok "服务注册完成: ${service_name}.service"
}

# ── 等待数据库就绪 ────────────────────────────────────────────────────────────────
_dm_wait_ready() {
    local disql_bin="$DM_INSTALL_PATH/bin/disql"
    local conn="SYSDBA/${SYSDBA_PWD}@localhost:${DM_PORT}"
    local attempt=1
    while [ "$attempt" -le 30 ]; do
        if printf 'exit;\n' | "$disql_bin" "$conn" >/dev/null 2>&1; then
            return 0
        fi
        attempt=$((attempt + 1))
        [ "$attempt" -le 30 ] && sleep 2
    done
    log_err "数据库未在 60 秒内就绪，请检查服务状态"
    exit 1
}

# ── 开启归档模式 ──────────────────────────────────────────────────────────────────
enable_archivelog() {
    local disql_bin="$DM_INSTALL_PATH/bin/disql"
    local conn="SYSDBA/${SYSDBA_PWD}@localhost:${DM_PORT}"

    log_info "等待数据库就绪..."
    _dm_wait_ready

    # 幂等：查 V$DM_ARCH_INI，路径/文件大小/空间上限全部一致才跳过
    if {
        echo "SELECT ARCH_DEST FROM V\$DM_ARCH_INI WHERE ARCH_TYPE='LOCAL' AND ARCH_DEST='${DM_ARCH_PATH}' AND ARCH_FILE_SIZE=${DM_ARCH_FILE_SIZE} AND ARCH_SPACE_LIMIT=${DM_ARCH_SPACE_LIMIT};"
        echo "exit;"
    } | "$disql_bin" "$conn" 2>/dev/null | grep -qF "$DM_ARCH_PATH"; then
        log_info "归档配置已一致，跳过重复开启"
        return 0
    fi

    log_info "开启归档模式..."
    {
        echo "alter database mount;"
        echo "alter database archivelog;"
        echo "alter database add archivelog 'TYPE=LOCAL,DEST=${DM_ARCH_PATH},FILE_SIZE=${DM_ARCH_FILE_SIZE},SPACE_LIMIT=${DM_ARCH_SPACE_LIMIT}';"
        echo "alter database open;"
        echo "exit;"
    } | "$disql_bin" "$conn" || {
        log_err "开启归档模式失败，请查看上方 disql 输出"
        exit 1
    }
    log_ok "归档模式已开启: $DM_ARCH_PATH"
}

# ── 完成提示 ──────────────────────────────────────────────────────────────────────
print_success() {
    cat <<EOF

${GREEN}✓ 达梦数据库安装完成${RESET}

  安装路径  : $DM_INSTALL_PATH
  数据路径  : $DM_DATA_PATH
  监听端口  : $DM_PORT

╔══════════════════════════════════════════════════╗
║              达梦数据库初始凭证                  ║
╠══════════════════════════════════════════════════╣
║  SYSDBA     密码: $(printf '%-32s' "$SYSDBA_PWD")║
║  SYSAUDITOR 密码: $(printf '%-32s' "$SYSAUDITOR_PWD")║
╠══════════════════════════════════════════════════╣
║  首次登录后请立即修改密码                        ║
╚══════════════════════════════════════════════════╝

  连接测试  : $DM_INSTALL_PATH/bin/disql SYSDBA/"$SYSDBA_PWD"@localhost:$DM_PORT
  查看状态  : systemctl status DmService${DM_INSTANCE}.service

EOF
}

# ── 主流程 ───────────────────────────────────────────────────────────────────────
main() {
    check_root
    check_existing_install
    check_deps
    check_port
    check_memory
    check_install_disk
    create_dmdba_user
    setup_env
    SYSDBA_PWD=$(generate_password)
    SYSAUDITOR_PWD=$(generate_password)
    detect_platform
    select_download_url
    download_and_extract
    write_response_xml
    run_dminstall
    run_dminit
    write_dmarch_ini
    register_service
    enable_archivelog
    print_success
}

main
