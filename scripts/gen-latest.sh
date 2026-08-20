#!/usr/bin/env bash
# 生成 Tauri updater 的 latest.json（自建 Update Server 用）。
# 用法：bash scripts/gen-latest.sh <version> <签名> <更新包URL>
#   - version: 新版本号（如 0.1.4）
#   - 签名:    tauri signer 生成的文件签名（base64）
#   - 更新包URL: 自建服务器上的 .msi/.exe 下载地址
#
# 产出 stdout：标准 latest.json（platforms 按需扩展）。
# 参考：https://v2.tauri.app/plugin/updater/
#
# 例（Windows NSIS）：
#   bash scripts/gen-latest.sh 0.1.4 "dW50cnV..." "https://your-host/update/BoenMind_0.1.4_x64-setup.exe"
set -euo pipefail

VERSION="${1:?version required}"
SIGNATURE="${2:?signature required}"
URL="${3:?update url required}"

cat <<JSON
{
  "version": "${VERSION}",
  "notes": "BoenMind v${VERSION}",
  "pub_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "platforms": {
    "windows-x86_64": {
      "signature": "${SIGNATURE}",
      "url": "${URL}"
    }
  }
}
JSON