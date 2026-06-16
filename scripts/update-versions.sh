#!/usr/bin/env bash
# 通过 eco.dameng.com API 获取 DM8 各 CPU/OS 平台下载链接，写入 versions.txt
set -euo pipefail

API_BASE="https://eco.dameng.com/eco-download-server"
CDN_BASE="https://download.dameng.com"
OUT_FILE="${1:-$(cd "$(dirname "$0")/.." && pwd)/versions.txt}"

# 日志输出到 stderr，不影响 stdout 重定向到文件
log()  { printf "[%s] -- %s\n"  "$(date -u +%H:%M:%S)" "$*" >&2; }
ok()   { printf "[%s] OK %s\n"  "$(date -u +%H:%M:%S)" "$*" >&2; }
fail() { printf "[%s] ERR %s\n" "$(date -u +%H:%M:%S)" "$*" >&2; exit 1; }

# 从单行 JSON 提取第一个匹配 key 的字符串值
json_val() { grep -o "\"$1\":\"[^\"]*\"" | head -1 | sed "s/\"$1\":\"//;s/\"$//"; }

# 平台列表：cpuId osId arch cpu_key os_key 中文标注
# cpu_key / os_key 供 install.sh 运行时检测匹配
# 跳过 Win_32 (65) / Docker (35)
PLATFORMS=(
    "0   10  x86_64      x86      rhel7        X86 RHEL7"
    "0   8   x86_64      x86      rhel6        X86 RHEL6"
    "0   105 x86_64      x86      centos7      X86 CentOS7"
    "0   106 x86_64      x86      kylin10_sp3  X86 麒麟10-SP3"
    "0   107 x86_64      x86      ubuntu22     X86 Ubuntu22"
    "6   9   x86_64      hygon    nfsc         海光 中科方德"
    "6   14  x86_64      hygon    kylin10      海光 麒麟10"
    "6   15  x86_64      hygon    uos20        海光 统信UOS20"
    "3   25  aarch64     ft2000   kylin10_sp1  飞腾2000 麒麟10-SP1"
    "4   25  aarch64     kunpeng  kylin10_sp1  鲲鹏920 麒麟10-SP1"
    "4   15  aarch64     kunpeng  uos20        鲲鹏920 统信UOS20"
    "5   14  mips64el    ls4000   kylin10      龙芯4000 麒麟10"
    "41  14  loongarch64 ls5000   kylin10      龙芯5000 麒麟10"
    "41  15  loongarch64 ls5000   uos20        龙芯5000 统信UOS20"
    "98  14  sw_64       sw3231   kylin10      申威3231 麒麟10"
)

# ── 1. 获取当前 DM8 版本号 ────────────────────────────────────────────────────────
log "请求 eco.dameng.com 平台列表..."
page_resp=$(curl -sf --max-time 15 "$API_BASE/cpu/os/table/page/data") \
    || fail "无法访问 eco.dameng.com，检查网络连接"

db_version=$(printf '%s' "$page_resp" | json_val "dbVersion") \
    || fail "响应中未找到 dbVersion 字段"
[ -n "$db_version" ] || fail "dbVersion 为空"
ok "DM8 dbVersion = ${db_version}"

# ── 2. 逐平台请求下载链接，写入 versions.txt ────────────────────────────────────
log "逐平台请求下载链接..."
found=0
failed=0

{
    printf "# DM8 安装包下载地址（由 scripts/update-versions.sh 自动生成）\n"
    printf "# updated: %s\n" "$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
    printf "# 格式: arch<TAB>cpu<TAB>os<TAB>url<TAB>sha256\n"
    printf "# sha256 为 '-' 表示暂无校验和，填入 64 位十六进制字符串后自动启用下载校验\n"

    for entry in "${PLATFORMS[@]}"; do
        cpu_id=$(awk '{print $1}' <<< "$entry")
        os_id=$(awk  '{print $2}' <<< "$entry")
        arch=$(awk   '{print $3}' <<< "$entry")
        cpu_key=$(awk '{print $4}' <<< "$entry")
        os_key=$(awk  '{print $5}' <<< "$entry")
        label=$(awk   '{for(i=6;i<=NF;i++) printf "%s%s",$i,(i<NF?" ":""); print ""}' <<< "$entry")

        log "  ${arch}  ${cpu_key}/${os_key}  (${label})"

        resp=$(curl -sf --max-time 10 \
            "$API_BASE/cpu/os/table/download/$db_version/$cpu_id/$os_id") || {
            log "    => SKIP: API 无响应"
            failed=$((failed + 1))
            continue
        }

        rel_url=$(printf '%s' "$resp" | json_val "url")
        if [ -z "$rel_url" ]; then
            msg=$(printf '%s' "$resp" | json_val "message")
            log "    => SKIP: ${msg:-无 url 字段}"
            failed=$((failed + 1))
            continue
        fi

        full_url="${CDN_BASE}${rel_url}"
        log "    => $(basename "$rel_url")"
        printf "%s\t%s\t%s\t%s\t-\n" "$arch" "$cpu_key" "$os_key" "$full_url"
        found=$((found + 1))
    done
} >"${OUT_FILE}"

ok "已写入 ${OUT_FILE}（${found} 个平台${failed:+，${failed} 个跳过}）"
