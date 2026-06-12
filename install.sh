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

# versions.json 地址（替换为你的实际 raw URL）
VERSIONS_URL="https://raw.githubusercontent.com/guangluo/dm-installer/main/versions.json"

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
        log_err "缺少依赖: ${missing[*]}"
        exit 1
    fi
}

# ── 架构检测 ──────────────────────────────────────────────────────────────────────
detect_arch() {
    MACHINE=$(uname -m)
    log_info "检测到架构: $MACHINE"
}

# ── 从 versions.json 选取下载链接 ─────────────────────────────────────────────────
select_download_url() {
    log_info "获取下载链接列表..."
    local versions_data
    versions_data=$(curl -sf --max-time 15 "$VERSIONS_URL") || {
        log_err "无法获取 versions.json: $VERSIONS_URL"
        exit 1
    }

    # versions.json 格式: "arch": "https://..."（每行一条）
    # 先找含当前 arch 的行，再从中提取 https:// URL
    DOWNLOAD_URL=$(printf '%s' "$versions_data" \
        | grep "\"${MACHINE}\"" \
        | grep -o 'https://[^"]*')

    if [ -z "$DOWNLOAD_URL" ]; then
        log_err "versions.json 中无 ${MACHINE} 平台（支持: x86_64 aarch64 loongarch64 mips64el sw_64）"
        exit 1
    fi

    log_ok "下载链接: $DOWNLOAD_URL"
}

# ── 下载并解压 ────────────────────────────────────────────────────────────────────
download_and_extract() {
    TMPDIR_WORK=$(mktemp -d)
    local zip_file="$TMPDIR_WORK/dm8.zip"
    local extract_dir="$TMPDIR_WORK/dm8_extract"
    mkdir -p "$extract_dir"

    log_info "下载安装包..."
    curl -L --progress-bar -o "$zip_file" \
        --max-time 1800 --retry 3 \
        "$DOWNLOAD_URL" || {
        log_err "下载失败: $DOWNLOAD_URL"
        exit 1
    }
    log_ok "下载完成"

    log_info "解压安装包..."
    unzip -q "$zip_file" -d "$extract_dir"

    DM_INSTALL_BIN=$(find "$extract_dir" -name "DMInstall.bin" -type f | head -1)
    if [ -z "$DM_INSTALL_BIN" ]; then
        log_err "未在安装包中找到 DMInstall.bin"
        exit 1
    fi
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
    log_info "执行静默安装..."
    "$DM_INSTALL_BIN" -q "$RESPONSE_XML" || {
        log_err "DMInstall.bin 安装失败"
        exit 1
    }
    log_ok "安装完成"
}

# ── 初始化数据库 ──────────────────────────────────────────────────────────────────
run_dminit() {
    local dminit_bin="$DM_INSTALL_PATH/bin/dminit"
    [ -x "$dminit_bin" ] || { log_err "dminit 不存在: $dminit_bin"; exit 1; }

    log_info "初始化数据库实例..."
    "$dminit_bin" \
        "PATH=$DM_DATA_PATH" \
        "DB_NAME=$DM_DB_NAME" \
        "INSTANCE_NAME=$DM_INSTANCE" \
        "PORT_NUM=$DM_PORT" \
        "PAGE_SIZE=$DM_PAGE_SIZE" \
        "EXTENT_SIZE=$DM_EXTENT_SIZE" \
        "CASE_SENSITIVE=$DM_CASE_SENSITIVE" \
        "CHARSET=$DM_CHARSET" || {
        log_err "dminit 初始化失败"
        exit 1
    }
    log_ok "数据库初始化完成"
}

# ── 注册 systemd 服务 ────────────────────────────────────────────────────────────
register_service() {
    local service_script="$DM_INSTALL_PATH/script/dm_service_installer.sh"
    if [ ! -f "$service_script" ]; then
        log_warn "未找到 dm_service_installer.sh，跳过服务注册"
        return 0
    fi

    log_info "注册 systemd 服务..."
    bash "$service_script" -t dmserver \
        -p "$DM_INSTALL_PATH/bin/dmserver" \
        -n "$DM_INSTANCE" \
        -d "$DM_DATA_PATH" || {
        log_err "服务注册失败"
        exit 1
    }

    local service_name="DmService${DM_INSTANCE}.service"
    systemctl enable "$service_name" \
        || log_warn "请手动执行: systemctl enable $service_name"
    systemctl start "$service_name" \
        || log_warn "服务启动失败，请检查: journalctl -u $service_name"
    log_ok "服务注册完成: $service_name"
}

# ── 完成提示 ──────────────────────────────────────────────────────────────────────
print_success() {
    cat <<EOF

${GREEN}✓ 达梦数据库安装完成${RESET}

  安装路径  : $DM_INSTALL_PATH
  数据路径  : $DM_DATA_PATH
  监听端口  : $DM_PORT

  连接测试  : $DM_INSTALL_PATH/bin/disql SYSDBA/SYSDBA@localhost:$DM_PORT
  查看状态  : systemctl status DmService${DM_INSTANCE}.service

EOF
}

# ── 主流程 ───────────────────────────────────────────────────────────────────────
main() {
    check_root
    check_existing_install
    check_deps
    detect_arch
    select_download_url
    download_and_extract
    write_response_xml
    run_dminstall
    run_dminit
    register_service
    print_success
}

main
