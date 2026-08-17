// M3 收尾验证：respond pending 表 / goal.* / session.projection / attachment /
// updateQueue / openDocument / openPath / subagent.* / session.export / agentPreset 5 法。
// 前置：web-server 以 BM_TEST_HOOKS=1 启动（测试钩子注入 pending 条目）。
// 用法：node docs/conformance/m3-r3-verify.mjs [port]
const PORT = process.argv[2] || "3079";
const API = `http://127.0.0.1:${PORT}/api`;
let failures = 0;
const ok = (name, cond, extra = "") => { console.log(cond ? "PASS" : "FAIL", name, extra); if (!cond) failures++; };

async function rpc(method, payload) {
  const r = await fetch(`${API}/${method}`, { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ type: "client-request", rpcId: `m3r3-${method}`, method, payload }) });
  return r.json();
}
async function respond(payload) {
  const r = await fetch(`${API}/respond`, { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ type: "client-response", rpcId: payload.rpcId, result: payload.result }) });
  return r.json();
}

// 1. session.create + goal 全链路
const s = await rpc("session.create", {});
const sid = s.result.value.sessionId;
ok("session.create", s.result.ok, sid);

// 2. goal.create → 投影在 history tail
const gc = await rpc("goal.create", { sessionId: sid, objective: "build the thing", maxGoalRounds: 5 });
ok("goal.create", gc.result.ok, JSON.stringify(gc.result.value));
const goalRef = gc.result.value.ref;
const gh = await rpc("session.history", { sessionId: sid });
const projGoal = gh.result.value.projections.values.goal;
ok("history tail projections has goal", projGoal && projGoal.goal.objective === "build the thing" && projGoal.goal.phase === "active",
  JSON.stringify(projGoal?.goal));
ok("projections asOfSeq", gh.result.value.projections.asOfSeq >= -1);

// 3. goal.edit（revision 递增）
const ge = await rpc("goal.edit", { sessionId: sid, ref: goalRef, objective: "build the thing better" });
ok("goal.edit", ge.result.ok && ge.result.value.ref.revision === goalRef.revision + 1, JSON.stringify(ge.result.value));

// 4. goal.pause / resume / complete
const gp = await rpc("goal.pause", { sessionId: sid, ref: ge.result.value.ref });
ok("goal.pause", gp.result.ok, JSON.stringify(gp.result.value));
const gh2 = await rpc("session.history", { sessionId: sid });
ok("goal paused in projection", gh2.result.value.projections.values.goal.goal.phase === "paused");
const gr = await rpc("goal.resume", { sessionId: sid, ref: gp.result.value.ref });
ok("goal.resume", gr.result.ok);
const gcom = await rpc("goal.complete", { sessionId: sid, ref: gr.result.value.ref });
ok("goal.complete", gcom.result.ok);

// 5. goal CAS 冲突 → 逐字错误码
const gbad = await rpc("goal.complete", { sessionId: sid, ref: goalRef });
ok("goal stale ref conflict", !gbad.result.ok && gbad.result.error.code === "goal-conflict",
  JSON.stringify(gbad.result.error));

// 6. goal.clear → 墓碑（投影 null）
const gcl = await rpc("goal.clear", { sessionId: sid, ref: gcom.result.value.ref });
ok("goal.clear", gcl.result.ok && gcl.result.value.cleared === true);
const gh3 = await rpc("session.history", { sessionId: sid });
ok("goal tombstone null projection", gh3.result.value.projections.values.goal === null);

// 7. session/attachment：无引用 → attachment-error；updateQueue 未知 item → queue-item-not-found
const att = await rpc("session.attachment", { sessionId: sid, attachmentId: "att_1" });
ok("session.attachment no-ref error", !att.result.ok && att.result.error.code === "attachment-error");
const uq = await rpc("session.updateQueue", { sessionId: sid, itemId: "item_x", action: { kind: "remove" } });
ok("session.updateQueue unknown item", !uq.result.ok && uq.result.error.code === "queue-item-not-found");

// 8. settings.openDocument / host.openPath
const od = await rpc("settings.openDocument", {});
ok("settings.openDocument", od.result.ok && od.result.value.opened === true);
const op = await rpc("host.openPath", { path: "." });
ok("host.openPath", op.result.ok && typeof op.result.value.opened === "boolean", JSON.stringify(op.result.value));
const opb = await rpc("host.openPath", { path: "" });
ok("host.openPath empty rejected", !opb.result.ok && opb.result.error.code === "bad-request");

// 9. subagent.* 空态：parent 可用时 list 返回 parentAvailable:false；未知 parent 逐字错误码
const sa = await rpc("subagent.list", { parentSessionId: sid });
ok("subagent.list parentAvailable false", sa.result.ok && sa.result.value.parentAvailable === false && Array.isArray(sa.result.value.entries));
const sa2 = await rpc("subagent.history", { parentSessionId: sid, childSessionId: "c1", mode: "one-shot" });
ok("subagent.history unassembled", !sa2.result.ok && sa2.result.error.code === "subagent-parent-unavailable");
const sa3 = await rpc("subagent.prompt", { parentSessionId: sid, childSessionId: "c1", mode: "continuable", content: [{ type: "text", text: "go" }] });
ok("subagent.prompt unassembled", !sa3.result.ok && sa3.result.error.code === "subagent-parent-unavailable");
const sa4 = await rpc("subagent.list", { parentSessionId: "no-such" });
ok("subagent.list unknown parent", !sa4.result.ok && sa4.result.error.code === "subagent-parent-unavailable");

// 10. agentPreset.* 剩余 5 法
const ps = await rpc("agentPreset.select", { sessionId: sid, agentPreset: "architect" });
ok("agentPreset.select blank ok path", !ps.result.ok && ps.result.error.code === "agent-preset-not-found");
const pr = await rpc("agentPreset.read", { agentPreset: "architect" });
ok("agentPreset.read", !pr.result.ok && pr.result.error.code === "agent-preset-not-found");
const pc = await rpc("agentPreset.copy", { from: "architect", agentPreset: "my-architect" });
ok("agentPreset.copy", !pc.result.ok && pc.result.error.code === "agent-preset-not-found");
const pod = await rpc("agentPreset.openDocument", { agentPreset: "architect" });
ok("agentPreset.openDocument", !pod.result.ok && pod.result.error.code === "agent-preset-not-found");
const prm = await rpc("agentPreset.remove", { agentPreset: "architect" });
ok("agentPreset.remove", !prm.result.ok && prm.result.error.code === "agent-preset-not-found");

// 11. respond 全链路：注入 approval pending → 三分支
const regA = await rpc("_test.registerApproval", { sessionId: sid, approvalId: "ap_1", toolName: "write_file", rpcId: "stable-approval-1" });
ok("test registerApproval", regA.result.ok, JSON.stringify(regA.result.value));
const apRpcId = regA.result.value.rpcId;
// 11a. not-pending
const np = await respond({ rpcId: "no-such-pending", result: { ok: true, value: {} } });
ok("respond not-pending", np.accepted === false && np.reason === "not-pending", JSON.stringify(np));
// 11b. bad-response（mismatched approvalId）
const br = await respond({ rpcId: apRpcId, result: { ok: true, value: { sessionId: sid, approvalId: "wrong", outcome: "allowed-once" } } });
ok("respond bad-response (mismatch)", br.accepted === false && br.reason === "bad-response", JSON.stringify(br));
// 11c. accepted（correct）
const acc = await respond({ rpcId: apRpcId, result: { ok: true, value: { sessionId: sid, approvalId: "ap_1", outcome: "allowed-once" } } });
ok("respond approval accepted", acc.accepted === true, JSON.stringify(acc));
// 11d. 已消费 → not-pending
const np2 = await respond({ rpcId: apRpcId, result: { ok: true, value: { sessionId: sid, approvalId: "ap_1", outcome: "allowed-once" } } });
ok("respond consumed → not-pending", np2.accepted === false && np2.reason === "not-pending");

// 12. question pending：整批校验
const regQ = await rpc("_test.registerQuestion", {
  sessionId: sid, rpcId: "stable-question-1",
  questions: [{ id: "q1", question: "proceed?", options: [{ label: "yes" }, { label: "no" }] }],
});
ok("test registerQuestion", regQ.result.ok);
const qRpcId = regQ.result.value.rpcId;
// 12a. 数量不匹配 → bad-response
const qBad = await respond({ rpcId: qRpcId, result: { ok: true, value: { sessionId: sid, answer: { answers: [] } } } });
ok("question batch mismatch bad-response", qBad.accepted === false && qBad.reason === "bad-response", JSON.stringify(qBad));
// 12b. selected 不在 option 集合 → bad-response
const qBad2 = await respond({ rpcId: qRpcId, result: { ok: true, value: { sessionId: sid, answer: { answers: [{ id: "q1", selected: ["maybe"] }] } } } });
ok("question unknown label bad-response", qBad2.accepted === false && qBad2.reason === "bad-response");
// 12c. 正确 → accepted
const qOk = await respond({ rpcId: qRpcId, result: { ok: true, value: { sessionId: sid, answer: { answers: [{ id: "q1", selected: ["yes"] }] } } } });
ok("question accepted", qOk.accepted === true, JSON.stringify(qOk));
// 12d. ok:false + cancelled → accepted（用户取消）
const regQ2 = await rpc("_test.registerQuestion", {
  sessionId: sid, rpcId: "stable-question-2",
  questions: [{ id: "q2", question: "continue?" }],
});
const qCancel = await respond({ rpcId: regQ2.result.value.rpcId, result: { ok: false, error: { code: "cancelled", message: "nope" } } });
ok("question cancelled accepted", qCancel.accepted === true, JSON.stringify(qCancel));
// 12e. ok:false 非 cancelled → bad-response
const regQ3 = await rpc("_test.registerQuestion", {
  sessionId: sid, rpcId: "stable-question-3",
  questions: [{ id: "q3", question: "continue?" }],
});
const qBad3 = await respond({ rpcId: regQ3.result.value.rpcId, result: { ok: false, error: { code: "boom", message: "x" } } });
ok("question non-cancel error bad-response", qBad3.accepted === false && qBad3.reason === "bad-response");

// 13. mux 重开重放 pending 帧（rpcId 原样复用）
const regA2 = await rpc("_test.registerApproval", { sessionId: sid, approvalId: "ap_2", toolName: "run_shell", rpcId: "replay-approval" });
ok("registerApproval for replay", regA2.result.ok);
function collectFrames(url, ms) {
  return new Promise((resolve, reject) => {
    const frames = [];
    const ws = new WebSocket(url);
    let timer = null;
    const stop = (ok) => { clearTimeout(timer); try { ws.close(); } catch {} resolve(ok); };
    ws.addEventListener("message", (e) => {
      frames.push(JSON.parse(e.data.toString()));
      clearTimeout(timer);
      timer = setTimeout(() => stop(frames), ms);
    });
    ws.addEventListener("error", () => reject(new Error("ws error")));
    timer = setTimeout(() => stop(frames), ms);
  });
}
const muxFrames = await collectFrames(`ws://127.0.0.1:${PORT}/api/events.mux`, 500);
const replay = muxFrames.find(f => f.method === "approval/requested" && f.payload.approvalId === "ap_2");
ok("mux replay approval/requested with stable rpcId", !!replay && replay.rpcId === "replay-approval", JSON.stringify(replay?.rpcId));
// 重放后再消费 → resolved 帧（先开流监听，再 respond——广播不重放历史帧）。
const resolvedHit = await new Promise((resolve, reject) => {
  const frames = [];
  const ws = new WebSocket(`ws://127.0.0.1:${PORT}/api/events.mux`);
  let consumed = null;
  let timer = null;
  const hit = () => frames.some(f => f.method === "approval/resolved" && f.payload.approvalId === "ap_2" && f.payload.outcome === "rejected");
  const finish = (val) => { clearTimeout(timer); try { ws.close(); } catch {} resolve(val); };
  const t = setTimeout(() => finish({ accepted: consumed?.accepted === true, resolved: hit() }), 6000);
  timer = t;
  ws.addEventListener("message", (e) => {
    frames.push(JSON.parse(e.data.toString()));
    if (hit()) finish({ accepted: consumed?.accepted === true, resolved: true });
  });
  ws.addEventListener("error", () => reject(new Error("ws error")));
  ws.addEventListener("open", async () => {
    consumed = await respond({ rpcId: "replay-approval", result: { ok: true, value: { sessionId: sid, approvalId: "ap_2", outcome: "rejected" } } });
    if (hit()) finish({ accepted: consumed.accepted === true, resolved: true });
  });
});
ok("replay consume", resolvedHit.accepted === true);
ok("approval/resolved broadcast", resolvedHit.resolved === true);

// 14. session.export ZIP
const exp = await fetch(`${API}/session.export?sessionId=${sid}`, { method: "GET" });
ok("session.export status", exp.status === 200, `status ${exp.status}`);
ok("session.export content-type", exp.headers.get("content-type") === "application/zip");
const cd = exp.headers.get("content-disposition") || "";
ok("session.export disposition", cd.startsWith("attachment; filename=\"dsh-session-"));
const zipBytes = new Uint8Array(await exp.arrayBuffer());
ok("session.export non-empty", zipBytes.length > 0, `${zipBytes.length} bytes`);
// 解压（node 无内置 zip——用 magic 头 + 简单探查）
const magic = String.fromCharCode(zipBytes[0], zipBytes[1], zipBytes[2], zipBytes[3]);
ok("session.export zip magic PK", magic === "PK\u0003\u0004", magic);
// 非法 query → 400；缺 session → 404
const expBad = await fetch(`${API}/session.export?sessionId=${sid}&includeDescendants=maybe`, { method: "GET" });
ok("session.export bad includeDescendants 400", expBad.status === 400);
const exp404 = await fetch(`${API}/session.export?sessionId=no-such-session`, { method: "GET" });
ok("session.export missing session 404", exp404.status === 404);
const expNoQuery = await fetch(`${API}/session.export`, { method: "GET" });
ok("session.export no sessionId 400", expNoQuery.status === 400);

// 15. goal.* 无会话 → bad-request；RPC 计数
const gNoSid = await rpc("goal.create", { objective: "x" });
ok("goal.create missing sessionId", !gNoSid.result.ok && gNoSid.result.error.code === "bad-request");

console.log(failures === 0 ? "\n=== M3 R3 VERIFY PASS ===" : `\n=== M3 R3 VERIFY FAIL (${failures}) ===`);
process.exit(failures === 0 ? 0 : 1);
