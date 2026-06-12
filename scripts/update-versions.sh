#!/usr/bin/env bash
# 通过 eco.dameng.com API 获取各平台最新 DM8 下载链接，写入 versions.json
set -euo pipefail

API_BASE="https://eco.dameng.com/eco-download-server"
CDN_BASE="https://download.dameng.com"
OUT_FILE="${1:-$(cd "$(dirname "$0")/.." && pwd)/versions.json}"
TMPDIR_WORK=$(mktemp -d)
trap 'rm -rf "$TMPDIR_WORK"' EXIT

python_exec() {
    if command -v python3 >/dev/null 2>&1; then python3 "$@"
    else python "$@"; fi
}

# CPU ID → uname -m 架构
cpu_arch() {
    case "$1" in
        0)  echo "x86_64"      ;;  # X86
        3)  echo "aarch64"     ;;  # 飞腾2000
        4)  echo "aarch64"     ;;  # 鲲鹏920
        5)  echo "mips64el"    ;;  # 龙芯4000
        41) echo "loongarch64" ;;  # 龙芯5000
        98) echo "sw_64"       ;;  # 申威3231
        6)  echo "x86_64"      ;;  # 海光
        *)  echo "unknown"     ;;
    esac
}

echo "[--] 获取平台列表..."
curl -sf --max-time 15 "$API_BASE/cpu/os/table/page/data" \
    >"$TMPDIR_WORK/page.json" || {
    echo "[ERR] 无法访问 eco.dameng.com API" >&2; exit 1
}

# 从 page data 提取 db_version 和所有 cpu/os 组合（跳过 Docker cpuId=35）
python_exec - "$TMPDIR_WORK/page.json" "$TMPDIR_WORK/combos.tsv" <<'PYEOF'
import json, sys

with open(sys.argv[1]) as f:
    data = json.load(f)

db_version = data["result"]["dm8Data"][0]["dbVersion"]

rows = []
for block in data["result"]["dm8Data"]:
    for cpu in block["dataSelectInfos"]:
        if cpu["cpuId"] == "35":  # Docker — 跳过
            continue
        for os in cpu.get("osInfos", []):
            rows.append(f"{db_version}\t{cpu['cpuId']}\t{cpu['cpuName']}\t{os['id']}\t{os['name']}")

with open(sys.argv[2], "w") as f:
    f.write("\n".join(rows) + "\n")
PYEOF

echo "[--] 查询各平台下载链接..."
found=0
json_entries=""

while IFS=$'\t' read -r db_ver cpu_id cpu_name os_id os_name; do
    resp_file="$TMPDIR_WORK/resp_${cpu_id}_${os_id}.json"
    curl -sf --max-time 10 \
        "$API_BASE/cpu/os/table/download/$db_ver/$cpu_id/$os_id" \
        >"$resp_file" 2>/dev/null || continue

    rel_url=$(python_exec -c "
import json, sys
with open('$resp_file') as f: d = json.load(f)
print(d['result']['url'])
" 2>/dev/null) || continue
    [ -z "$rel_url" ] && continue

    arch=$(cpu_arch "$cpu_id")
    full_url="${CDN_BASE}${rel_url}"

    # 从文件名提取后缀作为 key（去掉 dm8_YYYYMMDD_ 前缀）
    filename=$(basename "$rel_url" .zip)
    suffix="${filename#dm8_????????_}"

    printf "  %-44s (%s)\n" "$cpu_name / $os_name" "$arch"

    sep="${json_entries:+,}"
    safe_cpu=$(python_exec -c "import json; print(json.dumps('$cpu_name'))")
    safe_os=$(python_exec  -c "import json; print(json.dumps('$os_name'))")
    json_entries="${json_entries}${sep}
    \"${suffix}\": {
      \"cpu_id\": \"${cpu_id}\",
      \"os_id\":  \"${os_id}\",
      \"cpu\":    ${safe_cpu},
      \"os\":     ${safe_os},
      \"arch\":   \"${arch}\",
      \"url\":    \"${full_url}\"
    }"
    found=$((found + 1))
done <"$TMPDIR_WORK/combos.tsv"

updated_at=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

cat >"$OUT_FILE" <<JSON
{
  "updated_at": "${updated_at}",
  "platforms": {${json_entries}
  }
}
JSON

echo "[OK] 已写入 ${OUT_FILE}，共 ${found} 个平台"
