#!/usr/bin/env bash
# 通过 eco.dameng.com API 获取各平台最新 DM8 下载链接，写入 versions.json
set -euo pipefail

API_BASE="https://eco.dameng.com/eco-download-server"
CDN_BASE="https://download.dameng.com"
OUT_FILE="${1:-$(cd "$(dirname "$0")/.." && pwd)/versions.json}"

log()  { printf "[%s] -- %s\n"  "$(date -u +%H:%M:%S)" "$*"; }
ok()   { printf "[%s] OK %s\n"  "$(date -u +%H:%M:%S)" "$*"; }
fail() { printf "[%s] ERR %s\n" "$(date -u +%H:%M:%S)" "$*" >&2; exit 1; }

# 从 JSON 字符串中提取第一个匹配 key 的字符串值（跨平台，不依赖 grep -P）
json_val() {
    grep -o "\"$1\":\"[^\"]*\"" | head -1 | sed "s/\"$1\":\"//;s/\"$//"
}

# 每个架构对应达梦官网的 cpuId / osId（preferred 优先级）
# 数据来自 /cpu/os/table/page/data 接口的 CPU/OS 枚举
cpu_for_arch() {
    case "$1" in
        x86_64)      echo "0"  ;;  # X86
        aarch64)     echo "4"  ;;  # 鲲鹏920（最主流 ARM 服务器）
        loongarch64) echo "41" ;;  # 龙芯5000（LoongArch 新架构）
        mips64el)    echo "5"  ;;  # 龙芯4000（MIPS 兼容）
        sw_64)       echo "98" ;;  # 申威3231
    esac
}

os_for_arch() {
    case "$1" in
        x86_64)      echo "10" ;;  # rhel7（兼容性最广）
        aarch64)     echo "25" ;;  # 麒麟10 SP1
        loongarch64) echo "14" ;;  # 麒麟10
        mips64el)    echo "14" ;;  # 麒麟10
        sw_64)       echo "14" ;;  # 麒麟10
    esac
}

label_for_arch() {
    case "$1" in
        x86_64)      echo "X86 / rhel7"           ;;
        aarch64)     echo "鲲鹏920 / 麒麟10 SP1"   ;;
        loongarch64) echo "龙芯5000 / 麒麟10"       ;;
        mips64el)    echo "龙芯4000 / 麒麟10"       ;;
        sw_64)       echo "申威3231 / 麒麟10"       ;;
    esac
}

# ── 1. 获取当前 DM8 版本号 ────────────────────────────────────────────────────────
log "请求 eco.dameng.com 平台列表..."
page_resp=$(curl -sf --max-time 15 "$API_BASE/cpu/os/table/page/data") \
    || fail "无法访问 eco.dameng.com，检查网络连接"

db_version=$(printf '%s' "$page_resp" | json_val "dbVersion") \
    || fail "响应中未找到 dbVersion 字段"
[ -n "$db_version" ] || fail "响应中未找到 dbVersion 字段"

ok "DM8 dbVersion = ${db_version}"

# ── 2. 逐架构获取下载链接 ─────────────────────────────────────────────────────────
log "逐架构请求下载链接..."
platforms_json=""
found=0
failed=0

for arch in x86_64 aarch64 loongarch64 mips64el sw_64; do
    cpu_id=$(cpu_for_arch "$arch")
    os_id=$(os_for_arch   "$arch")
    label=$(label_for_arch "$arch")

    log "  $arch ($label) — cpu=$cpu_id os=$os_id"

    resp=$(curl -sf --max-time 10 \
        "$API_BASE/cpu/os/table/download/$db_version/$cpu_id/$os_id") || {
        printf "       [SKIP] API 无响应\n"
        failed=$((failed + 1))
        continue
    }

    rel_url=$(printf '%s' "$resp" | json_val "url")
    if [ -z "$rel_url" ]; then
        printf "       [SKIP] 响应中无 url 字段: %s\n" "$resp"
        failed=$((failed + 1))
        continue
    fi

    full_url="${CDN_BASE}${rel_url}"
    printf "       => %s\n" "$(basename "$rel_url")"

    sep="${platforms_json:+,}"
    platforms_json="${platforms_json}${sep}
    \"${arch}\": \"${full_url}\""
    found=$((found + 1))
done

[ "$found" -eq 0 ] && fail "所有平台均获取失败"

# ── 3. 写入 versions.json ─────────────────────────────────────────────────────────
updated_at=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

cat >"${OUT_FILE}" <<JSON
{
  "updated_at": "${updated_at}",
  "platforms": {${platforms_json}
  }
}
JSON

ok "已写入 ${OUT_FILE}（${found} 个平台${failed:+，${failed} 个失败}）"
