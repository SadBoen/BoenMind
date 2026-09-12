#!/usr/bin/env bash
# 清空 .work/ —— 过程笔记暂存区。
#
# 用途:实现规格、评审/对比报告、会话草稿等一次性工件先落 .work/(gitignore),
# 结论熔入规范或台账后即可整体清空(ADR-0040 承接 ADR-0027"过程文档交付即删")。
# .work/ 永不入库,故本脚本只会删本地临时件;需要留存的内容请先熔进规范或 git 提交。
set -euo pipefail
cd "$(dirname "$0")/.."

if [ ! -d .work ]; then
  echo ".work/ 不存在,无需清理"
  exit 0
fi

echo "将删除 .work/ 下全部内容:"
du -sh .work 2>/dev/null || true
find .work -mindepth 1 -maxdepth 1 -print | sed 's/^/  /'

read -r -p "确认清空?[y/N] " ans
case "${ans:-N}" in
  y|Y) find .work -mindepth 1 -maxdepth 1 -exec rm -rf {} + && echo "已清空 .work/" ;;
  *) echo "已取消" ;;
esac
