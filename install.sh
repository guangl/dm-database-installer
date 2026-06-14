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
    if [ ${#missing[@]} -gt 0 ]; then
        log_err "缺少必要工具: ${missing[*]}"
        log_err "Debian/Ubuntu: apt-get install -y ${missing[*]}"
        log_err "RHEL/CentOS:   yum install -y ${missing[*]}"
        exit 1
    fi
}

# ── 创建 dmdba 系统用户 ───────────────────────────────────────────────────────────
create_dmdba_user() {
    if id dmdba >/dev/null 2>&1; then
        log_info "系统用户 dmdba 已存在，跳过创建"
        return 0
    fi
    log_info "创建系统用户 dmdba..."
    groupadd -r dinstall 2>/dev/null || true
    useradd -r -g dinstall -d /home/dmdba -m -s /bin/bash dmdba || {
        log_err "创建用户 dmdba 失败（需要 useradd 命令）"
        exit 1
    }
    log_ok "系统用户 dmdba 创建完成"
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
    RESPONSE_XML="$TMPDIR_WORK/dm_install.xml"
    cat >"$RESPONSE_XML" <<XML
<?xml version="1.0" encoding="utf-8"?>
<DATABASE>
    <INSTALL_TYPE>0</INSTALL_TYPE>
    <INSTALL_PATH>${DM_INSTALL_PATH}</INSTALL_PATH>
    <DM_DATA_PATH>${DM_DATA_PATH}</DM_DATA_PATH>
    <AUTO_OVERWRITE>0</AUTO_OVERWRITE>
    <AUTO_START>0</AUTO_START>
    <CREATE_DB_SERVICE>N</CREATE_DB_SERVICE>
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
        "ARCH_INI=1" \
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
    create_dmdba_user
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
    print_success
}

main
