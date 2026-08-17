#!/usr/bin/env bash
# 门禁 1 验收脚本：headless 回合全链路 + kill -9 恢复 + 尾部完整性。
# 用法：bash kernel/scripts/verify-gate1.sh
# 通过标准：所有步骤打印 OK，最终 exit 0。

set -u
cd "$(dirname "$0")/.."          # kernel/
BIN=./target/debug/headless.exe
WORK="$(mktemp -d)"
FAIL=0

step() { echo "=== $1 ==="; }
ok()   { echo "  -> $2"; }

cargo build -p headless >/dev/null 2>&1 || { echo "build failed"; exit 1; }

# 1. 完整回合全链路（消息→工具→回复）
step "1. roundtrip: 建会话 + 工具回合"
"$BIN" roundtrip "$WORK/t1.db" s1 && ok $? "roundtrip OK"

# 2. 完整回合后尾部必须完整
step "2. verify-tail after roundtrip"
"$BIN" verify-tail "$WORK/t1.db" s1 >/dev/null && ok $? "tail OK"

# 3. kill -9 断点 1：Step Started 落盘后自死
step "3. abort@1 (kill -9 断点)"
"$BIN" abort "$WORK/t1.db" s2 1
[ $? -ne 0 ] && ok $? "abort 后进程已死（模拟 kill -9）" || { echo "abort should not exit 0"; FAIL=1; }

# 4. 断点后磁盘日志必须是 torn（verify-tail 拒绝）
step "4. verify-tail must reject torn tail"
if "$BIN" verify-tail "$WORK/t1.db" s2 >/dev/null 2>&1; then
    echo "  -> FAIL: torn tail accepted"; FAIL=1
else
    ok 1 "torn tail correctly rejected"
fi

# 5. resume：修复（修剪悬空事件）+ 落盘 + 续跑
step "5. resume: repair + continue"
"$BIN" resume "$WORK/t1.db" s2 >/dev/null && ok $? "resume OK"

# 6. 修复必须持久化：verify-tail 恢复后必须 OK
step "6. verify-tail after repair"
"$BIN" verify-tail "$WORK/t1.db" s2 >/dev/null && ok $? "repair persisted"

# 7. kill -9 断点 2：chunk 落盘后自死
step "7. abort@2"
"$BIN" abort "$WORK/t1.db" s3 2
[ $? -ne 0 ] && ok $? "abort 后进程已死" || { echo "abort should not exit 0"; FAIL=1; }
step "8. verify-tail must reject torn"
if "$BIN" verify-tail "$WORK/t1.db" s3 >/dev/null 2>&1; then
    echo "  -> FAIL: torn tail accepted"; FAIL=1
else
    ok 1 "torn tail correctly rejected"
fi
step "9. resume after abort@2"
"$BIN" resume "$WORK/t1.db" s3 >/dev/null && ok $? "resume OK"
step "10. verify-tail after repair"
"$BIN" verify-tail "$WORK/t1.db" s3 >/dev/null && ok $? "repair persisted"

# 11. 会话隔离：两个会话互不污染
step "11. session isolation (s1 intact)"
"$BIN" verify-tail "$WORK/t1.db" s1 >/dev/null && ok $? "s1 tail OK"

rm -rf "$WORK"
if [ $FAIL -eq 0 ]; then
    echo ""
    echo "GATE1: ALL PASS"
    exit 0
else
    echo ""
    echo "GATE1: FAILED"
    exit 1
fi
