#!/usr/bin/env bash
# 达梦数据库单机静默安装脚本
# 用法: bash install.sh /path/to/dm.iso
set -euo pipefail

# ── 安装参数 ─────────────────────────────────────────────────────────────────────
DM_INSTALL_PATH="/home/dmdba/dmdbms"
DM_DATA_PATH="/home/dmdba/dmdbms/data/DAMENG"
DM_PORT=5236
DM_INSTANCE="DMSERVER"
DM_DB_NAME="DAMENG"
DM_PAGE_SIZE=32
DM_EXTENT_SIZE=32
DM_CHARSET=0        # 0=GB18030
DM_CASE_SENSITIVE=Y

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
MOUNT_DIR=""
TMPDIR_WORK=""

cleanup() {
    if [ -n "$MOUNT_DIR" ] && mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        umount "$MOUNT_DIR" 2>/dev/null || true
    fi
    [ -n "$TMPDIR_WORK" ] && rm -rf "$TMPDIR_WORK"
}
trap cleanup EXIT

# ── root 检查 ────────────────────────────────────────────────────────────────────
check_root() {
    if [ "$(id -u)" -ne 0 ]; then
        log_err "需要 root 权限，请以 root 或 sudo 运行"
        exit 1
    fi
}

# ── 幂等性检测 ───────────────────────────────────────────────────────────────────
check_existing_install() {
    if [ -f "$DM_INSTALL_PATH/bin/dmserver" ]; then
        log_warn "已检测到达梦实例（$DM_INSTALL_PATH/bin/dmserver 存在），跳过安装"
        exit 0
    fi
}

# ── 依赖检查 ─────────────────────────────────────────────────────────────────────
check_deps() {
    local missing=()
    command -v sha256sum >/dev/null 2>&1 || missing+=("sha256sum")
    if ! command -v bsdtar >/dev/null 2>&1 && ! command -v mount >/dev/null 2>&1; then
        missing+=("bsdtar 或 mount")
    fi
    if [ ${#missing[@]} -gt 0 ]; then
        log_err "缺少依赖: ${missing[*]}"
        exit 1
    fi
}

# ── 提取 ISO ─────────────────────────────────────────────────────────────────────
extract_iso() {
    local iso_path="$1"
    TMPDIR_WORK=$(mktemp -d)
    local extract_dir="$TMPDIR_WORK/dm_extract"
    mkdir -p "$extract_dir"

    log_info "提取安装包: $iso_path"
    if command -v bsdtar >/dev/null 2>&1; then
        bsdtar -xf "$iso_path" -C "$extract_dir" \
            || { log_err "bsdtar 提取失败"; exit 1; }
    else
        MOUNT_DIR="$TMPDIR_WORK/dm_mount"
        mkdir -p "$MOUNT_DIR"
        mount -o loop,ro "$iso_path" "$MOUNT_DIR" \
            || { log_err "mount 挂载 ISO 失败"; exit 1; }
        cp -r "$MOUNT_DIR/." "$extract_dir/"
        umount "$MOUNT_DIR"
        MOUNT_DIR=""
    fi

    DM_INSTALL_BIN=$(find "$extract_dir" -name "DMInstall.bin" -type f | head -1)
    if [ -z "$DM_INSTALL_BIN" ]; then
        log_err "未在安装包中找到 DMInstall.bin"
        exit 1
    fi
    chmod +x "$DM_INSTALL_BIN"
    log_ok "安装包提取完成"
}

# ── 生成 XML 响应文件 ────────────────────────────────────────────────────────────
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

# ── 执行静默安装 ──────────────────────────────────────────────────────────────────
run_dminstall() {
    log_info "执行 DMInstall.bin 静默安装..."
    "$DM_INSTALL_BIN" -q "$RESPONSE_XML" \
        || { log_err "DMInstall.bin 静默安装失败"; exit 1; }
    log_ok "DMInstall.bin 安装完成"
}

# ── 初始化数据库 (dminit) ────────────────────────────────────────────────────────
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
        "CHARSET=$DM_CHARSET" \
        || { log_err "dminit 初始化失败"; exit 1; }
    log_ok "数据库实例初始化完成"
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
        -d "$DM_DATA_PATH" \
        || { log_err "服务注册失败"; exit 1; }

    local service_name="DmService${DM_INSTANCE}.service"
    systemctl enable "$service_name" \
        || log_warn "systemctl enable 失败，请手动执行: systemctl enable $service_name"
    systemctl start "$service_name" \
        || log_warn "服务启动失败，请检查: journalctl -u $service_name"
    log_ok "systemd 服务注册完成: $service_name"
}

# ── 安装成功提示 ──────────────────────────────────────────────────────────────────
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
    local iso_path="${1:-}"
    if [ -z "$iso_path" ]; then
        log_err "用法: bash install.sh /path/to/dm.iso"
        exit 1
    fi
    if [ ! -f "$iso_path" ]; then
        log_err "安装包不存在: $iso_path"
        exit 1
    fi

    check_root
    check_existing_install
    check_deps
    extract_iso "$iso_path"
    write_response_xml
    run_dminstall
    run_dminit
    register_service
    print_success
}

main "$@"
