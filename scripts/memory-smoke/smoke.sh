#!/usr/bin/env bash
# 记忆注入真实冒烟（v0.20）：隔离 BOENMIND_HOME + 真实 MiniMax 模型
# ① 预置 facts.md → 聊天问"关于我你知道什么" → 回复应引用注入的事实
# ② 发「记住 xxx」→ facts.md 应新增事实（governance.memorize 链路）
# 用法: bash smoke.sh <release-bm-server.exe> <home-dir> <port>
set -u
EXE="$1"
HOME_DIR="$2"
PORT="$3"
BASE="http://127.0.0.1:${PORT}"

mkdir -p "${HOME_DIR}/.boenmind/memory"
cp ~/.boenmind/config.toml "${HOME_DIR}/.boenmind/config.toml"
cat > "${HOME_DIR}/.boenmind/memory/facts.md" <<'EOF'
## 事实
- 用户喜欢喝咖啡，每天三杯。
EOF

BOENMIND_HOME="${HOME_DIR}" RUST_LOG=bm_server=info,bm_loop=info \
  BOENMIND_PORT="${PORT}" "${EXE}" > "${HOME_DIR}/server.log" 2>&1 &
SRV=$!
trap 'kill $SRV 2>/dev/null' EXIT

for i in $(seq 1 30); do
  curl -s "${BASE}/api/health" >/dev/null 2>&1 && break
  sleep 1
done
echo "[ok] server up (pid $SRV)"

# ① 注入验证：问个人事实
SID=$(curl -s -X POST "${BASE}/api/sessions" -H "content-type: application/json" \
  -d '{"title":"memory-smoke"}' | sed -n 's/.*"id":"\([^"]*\)".*/\1/p')
echo "[session] $SID"

curl -s -N -X POST "${BASE}/api/chat" -H "content-type: application/json" \
  -d "{\"session_id\":\"${SID}\",\"message\":\"What personal facts do you know about me?\"}" \
  > "${HOME_DIR}/chat-inject.txt" 2>&1
echo "[①] inject reply bytes: $(wc -c < "${HOME_DIR}/chat-inject.txt")"
grep -o "咖啡" "${HOME_DIR}/chat-inject.txt" | head -1 | sed 's/^/[①] 回复含"咖啡"关键词: /'

# ② 记住指令 → facts.md 应新增（Windows curl 中文 JSON 会报 invalid unicode，用英文）
curl -s -N -X POST "${BASE}/api/chat" -H "content-type: application/json" \
  -d "{\"session_id\":\"${SID}\",\"message\":\"remember that my cat is named Pudding\"}" \
  > "${HOME_DIR}/chat-remember.txt" 2>&1
echo "[②] remember reply bytes: $(wc -c < "${HOME_DIR}/chat-remember.txt")"
echo "[②] facts.md 当前内容:"
cat "${HOME_DIR}/.boenmind/memory/facts.md"

# 收尾：SSE 通道挂着会占连接，服务停掉由 trap 处理
echo "[done] server log: ${HOME_DIR}/server.log"
