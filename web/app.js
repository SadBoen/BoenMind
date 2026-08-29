// BoenMind Web Surface 最小三视图(M3.4):会话 / 发送 / 事件流。
// 协议:POST /rpc/{method}(Bearer)+ GET /events/{sid}?since_seq=N(fetch 流式读,
// EventSource 无法携带鉴权头)。断线重连 = since 恢复到 lastSeen(resume cursor)。

const $ = (id) => document.getElementById(id);
const log = $("log");
let sessionId = null;
let lastSeen = 0;
let streaming = false;

function addMsg(cls, text) {
  const div = document.createElement("div");
  div.className = "msg " + cls;
  div.textContent = text;
  log.appendChild(div);
  log.scrollTop = log.scrollHeight;
}

function setState(s) { $("state").textContent = s; }

function headers() {
  return {
    "Authorization": "Bearer " + $("token").value.trim(),
    "Content-Type": "application/json",
  };
}

async function rpc(method, params) {
  const requestId = "web-" + crypto.randomUUID();
  const r = await fetch(`${$("url").value.trim()}/rpc/${method}`, {
    method: "POST",
    headers: headers(),
    body: JSON.stringify({ v: "0.1", method, request_id: requestId, params }),
  });
  if (r.status === 401) throw new Error("鉴权失败(401):检查令牌");
  const body = await r.json();
  if (!body.ok) throw new Error(body.error ? body.error.message : "未知错误");
  return body.result;
}

async function newSession() {
  try {
    setState("创建会话……");
    const r = await rpc("session.create", {
      agent: { name: "assistant", model_chain: ["zhipu.glm-4-flash"],
        budget: { max_tokens: 100000, max_turns: 20 } },
    });
    sessionId = r.session_id;
    localStorage.setItem("agent:" + r.session_id, r.agent_id);
    localStorage.setItem("lastSession", r.session_id);
    lastSeen = 0;
    log.innerHTML = "";
    $("sid").textContent = sessionId;
    addMsg("sys", "会话已创建:" + sessionId);
    $("input").disabled = false; $("send").disabled = false;
    setState("已连接");
    pump(); // 事件流循环
  } catch (e) { setState("错误:" + e.message); }
}

async function send() {
  const content = $("input").value.trim();
  if (!content || !sessionId) return;
  $("input").value = "";
  addMsg("user", content);
  try {
    const r = await rpc("agent.send_input", {
      session_id: sessionId, agent_id: currentAgentId(),
      content, input_trust: "trusted",
    });
    addMsg("sys", "回合已发起(op " + r.operation_id + "),等待回答……");
  } catch (e) { addMsg("sys", "发送失败:" + e.message); }
}

function currentAgentId() {
  return localStorage.getItem("agent:" + sessionId) || "";
}

// 事件流泵:fetch 流式读 SSE 帧;断线 2s 后以 lastSeen 重连(resume cursor 语义)
async function pump() {
  if (streaming) return;
  streaming = true;
  while (sessionId) {
    try {
      const r = await fetch(
        `${$("url").value.trim()}/events/${sessionId}?since_seq=${lastSeen}`,
        { headers: headers() }
      );
      if (r.status !== 200) throw new Error("HTTP " + r.status);
      setState("事件流已连接");
      const reader = r.body.getReader();
      const dec = new TextDecoder();
      let buf = "";
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += dec.decode(value, { stream: true });
        let idx;
        while ((idx = buf.indexOf("\n\n")) >= 0) {
          const frame = buf.slice(0, idx); buf = buf.slice(idx + 2);
          const dataLine = frame.split("\n").find((l) => l.startsWith("data: "));
          const idLine = frame.split("\n").find((l) => l.startsWith("id: "));
          if (!dataLine) continue;
          if (idLine) lastSeen = parseInt(idLine.slice(4), 10) || lastSeen;
          const env = JSON.parse(dataLine.slice(6));
          handleEvent(env);
        }
      }
    } catch (e) { setState("事件流断开:" + e.message); }
    if (!sessionId) break;
    setState("2s 后重连……");
    await new Promise((r2) => setTimeout(r2, 2000));
  }
  streaming = false;
}

function handleEvent(env) {
  const p = env.payload || {};
  switch (env.type) {
    case "session.created":
      localStorage.setItem("agent:" + env.payload.agent_id, env.payload.agent_id);
      break;
    case "agent.turn.started":
      if (p.agent_id === currentAgentId()) addMsg("user", "(已提交,处理中)");
      break;
    case "agent.completed":
      if (p.content) addMsg("bot", p.content);
      break;
    case "agent.failed":
      if (p.agent_id === currentAgentId()) addMsg("sys", "回合失败:" + (p.error_code || ""));
      break;
    default: break;
  }
}

$("newSession").onclick = newSession;
$("send").onclick = send;
$("input").addEventListener("keydown", (e) => { if (e.key === "Enter") send(); });

// 恢复上次会话(存在 localStorage 则仅恢复显示,ID 有效性由 resume 校验)
const last = localStorage.getItem("lastSession");
if (last) { $("sid").textContent = last + "(按「新会话」或刷新令牌后 resume)"; }
