# Code Review Questions

> BoenMind 微内核（DSH 核心 Rust 移植版）深度审查 — 2026-08-18
> 范围：8 个核心 crate 全部 .rs 文件（contracts/session/llm/tools/storage/loop/supervisor/assembly），
> headless 与 web-server 作为消费方壳层快速浏览（接口误用/契约违背视角）。
> Answer each question in the **Answer:** field below it.
> Use "intended behavior", "won't fix", or describe the desired fix.
> Then prompt me again to start implementing improvements.

---

## Architecture & Structure

### ARCH-001 (P2): Supervisor crate is never assembled into the runtime
**File(s):** `kernel-assembly/src/lib.rs:79`, `kernel-supervisor/src/lib.rs:86-197`
**Severity:** Medium
**Observation:** kernel-supervisor implements spawn/kill/restart with tests, but the composition root hardcodes `plugin_runtime: Arc::new(kernel_contracts::UnavailablePluginRuntime)` in both `headless` and web-server assembly paths. No code path constructs a `Supervisor`, so the whole crate is dead in every delivery profile despite the Cargo.toml description claiming "M3 完整".
**Question:** Is the supervisor intentionally unassembled (kept as a library for a later milestone), or is wiring it into `Runtime` missing? If unassembled, should `Runtime` expose an optional supervisor slot so consumers don't have to fork the composition root?
**Answer:**

---

### ARCH-002 (P3): Boundary guard omits web-server entirely
**File(s):** `kernel-assembly/tests/crate_boundaries.rs:16-25`
**Severity:** Low
**Observation:** `layer_of()` has no entry for `web-server`, and the test loop skips crates whose layer is unknown (line 56). The workspace has 10 members but web-server's dependency direction is completely unguarded. Today it only depends downward (contracts/session/loop/assembly/llm), so nothing is broken — but the guard's stated purpose ("依赖只许向下") has a hole any future consumer-shell drift could exploit silently.
**Question:** Add web-server to the layering table (layer 1, alongside headless)? Also the naive manifest parser (`workspace_deps`, lines 29-42) matches any line with `=` whose left side starts with `kernel-` — including comments — worth hardening to only parse `[dependencies]`/`[dev-dependencies]` sections?
**Answer:**

---

### ARCH-003 (P2): Composition root is not sealed — all Runtime fields are pub and the headless profile is assembled unusable
**File(s):** `kernel-assembly/src/lib.rs:37-48, 64-68`; `headless/src/main.rs:75, 86`
**Severity:** Medium
**Observation:** `Runtime::headless` assembles an empty-script `ScriptLlm` and an empty tool registry; every consumer must mutate public fields to make it work (`install_scripted_llm(&mut rt)` sets `rt.llm = ...`). A "组合根" whose invariants depend on consumers remembering to swap internals is fragile: `store`, `gate`, `tools` can be replaced out from under live agents.
**Question:** Should `Runtime` provide typed constructors (e.g., `with_llm`, `with_tools`) and make the fields read-only (getters + interior configuration), or is the open-fields design intentional for shell-layer convenience?
**Answer:**

---

### ARCH-004 (P2): create_session leaves a ghost in-memory session when persistence fails
**File(s):** `kernel-assembly/src/lib.rs:92-99`; `kernel-session/src/lib.rs:166-170`
**Severity:** Medium
**Observation:** `Runtime::create_session` inserts into `SessionStore` first (which emits `SessionStarted` on the bus and silently **replaces** any existing session with the same id), then calls `persist.create_session`. If persistence fails (e.g., duplicate id — sqlite returns "already exists"), the error propagates but the store keeps the new (replacement) session whose `SessionStarted` is not on disk. `SessionStore::create` also overwrites without any duplicate check.
**Question:** Should `SessionStore::create` reject duplicates (return Result), and should `Runtime::create_session` roll back the in-memory session when `persist.create_session` fails?
**Answer:**

---

### ARCH-005 (P3): session.fork bypasses the loop's logged-means-persisted discipline
**File(s):** `web-server/src/api.rs:611-694` (esp. 665-675)
**Severity:** Low
**Observation:** `session.fork` calls `agent.session().append(ev)` and then `runtime.persist.append_events(...)` manually per event — the same dual-write pattern as the headless `abort` mode (`headless/src/main.rs:148-175`). This duplicates the `ReactLoopAgent::persist` logic outside the loop crate; a persist failure mid-fork leaves a partially-copied fork session both in memory and on disk with no cleanup or turn-end marker.
**Question:** Should fork build the copied log and persist it as a single batch (storage supports batch append), with compensation (delete session) on failure?
**Answer:**

---

### ARCH-006 (P2): Adapter-level thinking/effort configuration is dead — settingsNs only syncs baseURL/apiKey
**File(s):** `kernel-llm/src/openai.rs:355-362`; `web-server/src/api.rs:1586-1596`
**Severity:** Medium
**Observation:** `build_request` calls `resolve_thinking(request, None, None)` — `adapter_thinking`/`adapter_effort` are always None, so the deployment-lock ("disabled") branch and adapter default-effort logic are unreachable. Meanwhile the settings namespace sync (`sync_provider_overrides`) only forwards `baseURL`; `thinking`/`reasoningEffort` settings writes land in the in-memory settings map but never reach the adapter.
**Question:** Is adapter-level thinking config (from config.toml / settings ns) planned? If yes, wire it; if no, remove the dead parameters from `resolve_thinking` to avoid false confidence in the deployment-lock behavior.
**Answer:**

---

### ARCH-007 (P3): Dead event vocabulary — SessionEnded / TurnEndReason::Interrupted / Blocked never produced
**File(s):** `kernel-contracts/src/session.rs:56-74, 112-114`; `kernel-loop/src/lib.rs` (no producer)
**Severity:** Low
**Observation:** The vocabulary declares `SessionEnded`, `TurnEndReason::Interrupted` (doc says "持久化后端在重载时关闭了崩溃孤儿回合"), and `Blocked`, but nothing ever appends them: the repair path truncates the tail instead of emitting `Interrupted`, and `delete_session` removes rows without any end event. The doc comment on `Interrupted` contradicts the actual truncation behavior.
**Question:** Either align the docs with the truncation strategy (remove/rename the variant) or emit `Interrupted` turn-end during repair. Which is intended?
**Answer:**

---

## Security

### SEC-001 (P2): /api/respond and /api/session.export bypass the trust fence entirely
**File(s):** `web-server/src/lib.rs:196-220` (respond), `web-server/src/lib.rs:305-370` (session.export); compare fence in `handle_rpc` at 74-89
**Severity:** Medium
**Observation:** The dual-fence (Host/Origin trust + privileged loopback-pin) is applied to `POST /api/{endpoint}` and the WS upgrade endpoints, but `POST /api/respond` and `GET /api/session.export` have **no** Host/Origin/sec-fetch-site checks at all. A DNS-rebinding attack that remaps a page origin to 127.0.0.1 could fetch `GET /api/session.export?sessionId=...` same-origin and read full session logs (zip) without any fence. Session ids are UUIDs, which mitigates guessing, but the fence's whole purpose is defense-in-depth and it is missing here.
**Question:** Should both handlers go through the same `is_trusted_api_request` gate as `handle_rpc` (and should session.export additionally be loopback-pinned or require a session id the client is attached to)?
**Answer:**

---

### SEC-002 (P2): Origin check ignores the port — cross-port localhost origins pass
**File(s):** `web-server/src/trust.rs:133-145` (`extract_url_host` drops port at line 143)
**Severity:** Medium
**Observation:** `extract_url_host` returns only the hostname, so an `Origin: http://127.0.0.1:<any-port>` passes against `Host: 127.0.0.1:3080`. DSH's reference logic compares WHATWG `new URL(origin).host`, which includes the port. This loosens the fence against any other local web app bound to another loopback port.
**Question:** Should the Origin comparison include the port (normalizing :80), matching DSH semantics?
**Answer:**

---

### SEC-003 (P2): host.openPath passes client text through cmd metacharacter parsing
**File(s):** `web-server/src/rpc_m3.rs:72-97` (esp. 81-83)
**Severity:** Medium
**Observation:** On Windows the path is executed as `cmd /C start "" <path>`. cmd re-parses the joined remainder as a command line, so a path containing `&` or `|` (e.g., `C:\x & calc`) is interpreted as command injection rather than a path. The method is privileged + loopback-pinned, which limits exposure, but a page running on loopback (or via the other fence gaps) could trigger arbitrary command execution through cmd metacharacters.
**Question:** Should openPath use `Command::new("cmd").raw_arg`/proper escaping, or better, invoke the OS open API directly (e.g., `ShellExecuteW` / `open` via a dedicated crate) instead of shelling out through `cmd`?
**Answer:**

---

### SEC-004 (P3): Test hooks gated only by env var presence
**File(s):** `web-server/src/api.rs:304-307, 315-318`
**Severity:** Low
**Observation:** `_test.registerApproval` / `_test.registerQuestion` become live when `BM_TEST_HOOKS=1` is set. There is no debug-build or explicit flag check beyond the env var; if the variable leaks into a production process environment, the pending-registry manipulation surface opens to any loopback page.
**Question:** Acceptable for this deployment profile, or should the hook also require `cfg(debug_assertions)` / a dedicated `--test-hooks` CLI flag?
**Answer:**

---

### SEC-005 (P3): API keys may be sent over plaintext HTTP
**File(s):** `web-server/src/provider_config.rs:145-148`; `kernel-llm/src/openai.rs:467-484`
**Severity:** Low
**Observation:** Config accepts `http://` base_urls (custom OpenAI-compatible endpoints), and the adapter sends `Authorization: Bearer <key>` over that transport. No warning or TLS enforcement exists for key-bearing custom endpoints.
**Question:** Should non-TLS base_urls be rejected or at least warn loudly when an API key is configured for them?
**Answer:**

---

### SEC-006 (P3): SSE line buffer grows without bound
**File(s):** `kernel-llm/src/openai.rs:552, 621` (`line_buf` has no size cap)
**Severity:** Low
**Observation:** A malicious or broken provider endpoint that streams a huge single "line" without `\n` will grow `line_buf` indefinitely (memory DoS). The endpoint is user-configured, so risk is bounded, but a cap (e.g., 1 MiB line / total response) with STREAM_CLOSED/TRANSPORT termination would be cheap insurance.
**Question:** Add a max line length / max buffered bytes guard to the SSE loop?
**Answer:**

---

## Performance

### PERF-001 (P2): One SQLite transaction + fsync per event, including every stream chunk
**File(s):** `kernel-loop/src/lib.rs:300-306` (persist per append), `kernel-storage/src/lib.rs:150-197`
**Severity:** Medium
**Observation:** `logged-means-persisted` is implemented as append→persist per event; each persist is its own transaction with `synchronous=FULL` under WAL, i.e., a full fsync. A 1,000-chunk model response = 1,000+ durability points per turn. The storage port already supports batch append (`append_events` takes a slice), so batching chunks per step (or per turn, with periodic flush) would preserve atomicity while removing the fsync amplification.
**Question:** Is per-event durability a hard requirement (kill -9 must not lose any single logged event), or can chunk events be batched into one transaction per stream step with a final flush before `Turn Ended`?
**Answer:**

---

### PERF-002 (P2): std::sync::Mutex held across sqlite fsync inside async methods
**File(s):** `kernel-storage/src/lib.rs:100-104` (`lock()`), 154-195 (`append_events`)
**Severity:** Medium
**Observation:** The single `Connection` is guarded by a blocking `std::sync::Mutex` and every operation runs to commit (fsync) while holding it — inside `async fn` on the tokio runtime. Concurrent sessions serialize on this one mutex and each holder blocks an executor thread for the duration of the fsync. Fine for a single-user desktop tool, but it is the systemic scalability ceiling and can stall other tasks under write bursts.
**Question:** Acceptable for the current single-user profile? If growth is expected, consider `tokio::task::spawn_blocking` for the DB work or a dedicated writer thread with an mpsc queue.
**Answer:**

---

### PERF-003 (P3): JSON Schema validator recompiled on every tool execution
**File(s):** `kernel-tools/src/lib.rs:72` (`jsonschema::validator_for` per `execute` call)
**Severity:** Low
**Observation:** `validator_for` compiles the schema on every tool call. Compilation is per-invocation overhead that could be cached on registration (or memoized per handler).
**Question:** Cache the compiled validator in the registry entry?
**Answer:**

---

### PERF-004 (P3): Full-log scans per step — O(n²) over long sessions
**File(s):** `kernel-loop/src/lib.rs:285-297` (`next_turn`), `kernel-session/src/lib.rs:127-151` (`derive_messages`)
**Severity:** Low
**Observation:** Each step re-scans/clones the entire event log (`next_turn` iterates all events; `derive_messages` clones every message each model call). Long sessions with many steps degrade quadratically. The turn number could be tracked in the agent, and message projection could be incremental.
**Question:** Worth incremental projection + cached turn counter now, or acceptable until sessions get long?
**Answer:**

---

### PERF-005 (P3): session.search loads and scans every session log per query
**File(s):** `web-server/src/api.rs:541-582`
**Severity:** Low
**Observation:** Search loads the complete event log of every persisted session into memory and scans it linearly per query — O(all sessions × total events) per keystroke-triggered search, no SQL FTS or index.
**Question:** Acceptable for the conformance subset, or plan to move search into SQLite (FTS5 or a text column scan)?
**Answer:**

---

## Code Quality & Refactoring

### QUAL-001 (P2): `.expect("translate")` on a function that cannot fail today
**File(s):** `kernel-llm/src/openai.rs:324` (`build_request`), `translate_messages` at 162-276
**Severity:** Medium
**Observation:** `translate_messages` returns `Result` but has no error path in the current enum (image blocks don't exist yet). `build_request` therefore `.expect()`s on an impossible failure — the moment an image/unsupported variant is added (the doc comment explicitly plans "image 块即拒收点"), the expect becomes a panic inside a library instead of the documented `UNSUPPORTED_CONTENT` finish.
**Question:** Propagate the `Result` from `build_request` into `stream_inner` and render failures as `FinishReason::Error{code:"UNSUPPORTED_CONTENT"}` instead of panicking?
**Answer:**

---

### QUAL-002 (P2): LlmError.retryable is dead — structured errors always retryable=false
**File(s):** `kernel-contracts/src/error.rs:106-167` (esp. 130-145)
**Severity:** Medium
**Observation:** `LlmError::structured()` hardcodes `retryable: false` even for RATE_LIMIT/SERVER/QUOTA classifications, and the only `retryable: true` producer is a transport error in `list_models_remote`. No retry policy exists yet, so the field is currently cosmetic — but any future backoff logic keyed on `retryable` will treat 429s as non-retryable.
**Question:** Should `structured()` derive `retryable` from the code/status (RATE_LIMIT, SERVER, TRANSPORT → true), or is retryability expected to live in a separate policy layer?
**Answer:**

---

### QUAL-003 (P2): Three copies of the tail-pairing algorithm
**File(s):** `kernel-assembly/src/lib.rs:185-225` (`repair_interrupted_turn`), `headless/src/main.rs:232-261` (`verify_tail`), `headless/src/main.rs:330-355` (`check_tail`)
**Severity:** Medium
**Observation:** The same "scan from tail, pair Step/Turn Started with Ended" logic is implemented three times with subtly different semantics (repair uses cut-and-truncate, verify uses boolean). They have already drifted (repair counts `turn_open`/`step_open` differently). This invariant is core to the event-log architecture and should exist exactly once, e.g., as a function in kernel-contracts or kernel-session consumed by both.
**Question:** Extract a single `is_tail_paired`/`trim_unpaired_tail` helper into a core crate and have assembly + headless + tests use it?
**Answer:**

---

### QUAL-004 (P2): `futures::executor::block_on` inside async request handlers
**File(s):** `web-server/src/rpc_m3.rs:263-278` (`parent_available`)
**Severity:** Medium
**Observation:** `parent_available` calls `futures::executor::block_on` to run a persist future inside an async context — it blocks a tokio worker thread (and nests a second executor). The persist port is async precisely to avoid this. It works only because the methods are called on a multi-thread runtime with low load.
**Question:** Make `parent_available` async and await the persist call directly (call sites in subagent.* are sync fns — they can become async like other handlers)?
**Answer:**

---

### QUAL-005 (P2): Event timestamps are written but never read — replay time fidelity is lost
**File(s):** `kernel-storage/src/lib.rs:92-95` (timestamp column), `kernel-assembly/src/lib.rs:141-147` (restore regenerates `Utc::now()`), `web-server/src/events.rs:36` (translate uses `Utc::now()`), `kernel-storage/src/lib.rs:272` (rewrite_events regenerates timestamps)
**Severity:** Medium
**Observation:** The `timestamp` column is persisted but never loaded (`load_events` returns only `event_json`), and both restore and wire translation regenerate `Utc::now()` — so history replay and session.export stamp every event with the replay time, and interrupted-turn repair rewrites original times. `SessionRecord::new` itself stamps `Utc::now()`, so even the in-memory log can't carry a persisted time.
**Question:** Should `SessionRecord` carry the stored timestamp through load/restore and `translate_events` use it for wire `time`?
**Answer:**

---

### QUAL-006 (P3): Catalog errors silently swallowed in resolve_model / MultiProviderLlm.list_models
**File(s):** `kernel-contracts/src/llm.rs:357-358` (`.unwrap_or_default()` on `list_models`), `kernel-llm/src/multi.rs:38-43` (unknown provider → `Ok(vec![])`)
**Severity:** Low
**Observation:** `resolve_model`'s default implementation swallows any `list_models` error and falls back to model-name-as-label; `MultiProviderLlm::list_models` returns an empty list for an unknown provider rather than an error (unlike `stream()`, which fail-louds with NO_ADAPTER). Callers can't distinguish "empty catalog" from "catalog failed".
**Question:** Intentional degradation, or should these surfaces return `LlmError` so RPC layers can report model-discovery failures?
**Answer:**

---

### QUAL-007 (P3): Hardcoded placeholders on the wire
**File(s):** `web-server/src/api.rs:397` (`updatedAt: "1970-01-01T00:00:00.000Z"`), `web-server/src/api.rs:142` (version "0.1.0"), `kernel-llm/src/openai.rs:31` (UA "boenmind/0.1.0")
**Severity:** Low
**Observation:** `session.list` reports a fixed epoch `updatedAt` although the storage layer maintains real `updated_at` values (used for ordering). The version/User-Agent strings are duplicated literals that will drift from Cargo.toml.
**Question:** Wire the real `updated_at` from storage into `session.list`, and derive version from `env!("CARGO_PKG_VERSION")`?
**Answer:**

---

### QUAL-008 (P3): Dead wiring — LoopRuntime.store unused; valid_endpoint/valid_channel unused
**File(s):** `kernel-loop/src/lib.rs:45` (`store` field never read by the agent), `web-server/src/rpc.rs:128-146` (only tests)
**Severity:** Low
**Observation:** `LoopRuntime.store` is constructed everywhere but never used inside kernel-loop (agents hold their session directly). `valid_channel`/`valid_endpoint` are implemented and tested but never called by the router (the `/api/{endpoint}` path is compared verbatim without charset validation).
**Question:** Remove the dead field, and either apply the endpoint validation in `handle_rpc` or drop the functions?
**Answer:**

---

### QUAL-009 (P3): ModelListEndpoint enum has one variant; minimax endpoint doc mismatch
**File(s):** `kernel-llm/src/openai.rs:44-47`; `web-server/src/main.rs:171` (always `Standard`); `web-server/src/provider_config.rs:52` (doc: minimax uses `GET /models/list`)
**Severity:** Low
**Observation:** The enum is a single-variant abstraction, and the provider_config docs claim MiniMax's list endpoint is `/models/list` while all providers are wired to the OpenAI-standard `GET /models`. If MiniMax actually differs, `llm.discoverModels` will fail for it.
**Question:** Which is correct for MiniMax — `/models` or `/models/list`? Drop the dead enum or implement the variant accordingly.
**Answer:**

---

### QUAL-010 (P3): Loose RPC envelope validation
**File(s):** `web-server/src/rpc.rs:19-27` (`ClientRequest.type_` never checked), `web-server/src/rpc_m3.rs:49-61` (`session_update_queue` ignores sessionId)
**Severity:** Low
**Observation:** The client-request envelope's `type` field is parsed but never validated to be `"client-request"`; `session_update_queue` discards the sessionId entirely and answers `queue-item-not-found` for any id. Both are honest enough today, but envelope laxness invites silent mismatches.
**Question:** Validate `type_ == "client-request"` (else bad-request), and confirm the updateQueue simplification matches the ledger intent?
**Answer:**

---

### QUAL-011 (P3): resolve_model is never consulted by the loop
**File(s):** `kernel-loop/src/lib.rs:366-378` (GenerateOptions built without resolve_model); `kernel-contracts/src/llm.rs:354-371`
**Severity:** Low
**Observation:** The loop hardcodes `temperature: None, max_tokens: None` and never calls `llm.resolve_model`, so `default_max_tokens`, context window and reasoning metadata (the whole point of `LlmResolvedModelInfo`) never materialize into requests. DSH's loop materializes adapter defaults into every request.
**Question:** Should `run_turn` resolve the model once per turn and apply `default_max_tokens`/reasoning defaults?
**Answer:**

---

## Bugs & Potential Issues

### BUG-001 (P0): Turn Started is never emitted — turn numbers are always 1 and blank-detection is broken
**File(s):** `kernel-loop/src/lib.rs:285-297` (`next_turn` scans for `Turn Started`), 312-336 (run_turn appends Step/chunks/Turn Ended but never `Turn(TurnEvent::Started)`); cascade: `web-server/src/main.rs:260-275` (`blank: !has_turn_start`), `web-server/src/events.rs:51-55` (turn/start translation)
**Severity:** Critical
**Observation:** `next_turn()` computes the next turn as max(Turn Started)+1, but no production code path ever appends `Turn(TurnEvent::Started {..})` (verified by grep: only pattern matches and tests construct it). Consequences: (1) every turn in a session is numbered turn=1 and steps restart at 1 — turn/step watermarking semantics of the DSH event waterfall are broken; (2) `turn/start` never appears on the wire; (3) web-server's restored-session blank detection (`blank: !has_turn_start`) is always `true`, so every restored session is mislabeled blank and `agentPreset.select`'s `agent-preset-locked` check misfires; (4) the `turn_open` branch of `repair_interrupted_turn` is dead. The doc comment "恢复续跑（turn 编号接续，不重复）" is contradicted by the code.
**Question:** Should `run_turn` append `Turn(TurnEvent::Started { turn })` immediately after the UserMessage (or before Step Started), and add a test asserting turn numbers increment across turns (e.g., turn 2 after a completed turn)?
**Answer:**

---

### BUG-002 (P1): Live wire seq restarts at 0 for restored sessions — post-restart events regress below lastSeq
**File(s):** `web-server/src/api.rs:222-244` (`attach_event_bus` per-session counter starts at 0), `web-server/src/main.rs:251-279` (restore loads events without replaying the bus), `web-server/src/ws.rs:28-49` (subscribed `lastSeq` from persisted wire length)
**Severity:** High
**Observation:** On startup, sessions are restored directly from `load_events` (no bus emit), so the listener's per-session seq counter stays at 0. The mux baseline advertises `lastSeq = (translated persisted events) - 1` (e.g., 8), but the first live event after a prompt gets `wire.seq = 0` — a regression below the watermark. Frontends that apply only `seq > lastSeq` will drop all post-restart live events for restored sessions. The comment at api.rs:219-221 ("每会话从 0 连续") only holds for sessions created within the current process lifetime.
**Question:** Seed the per-session live counter from the persisted log's translated wire length at restore time (or emit the restored log through the bus on restore), and add an integration test: restore a session with N events, prompt, assert the first live event seq == N?
**Answer:**

---

### BUG-003 (P2): EventBus::clone duplicates the listener id counter across shared slots
**File(s):** `kernel-contracts/src/bus.rs:64-73`
**Severity:** Medium
**Observation:** `Clone` copies the `slots` Arc but re-creates `next_id` as a **fresh** `AtomicU64` seeded from a Relaxed load of the original. Original and clones can then issue identical ids (two concurrent `on_event` calls on original+clone both get id 6). Since `slots` is shared, a `Disposer` from one bus clone drops the listener registered through the other clone (retain removes by id). Latent today because the web-server only registers on the original bus, but `Session`/`Runtime` hand out bus clones as public API.
**Question:** Make the id counter shared (`Arc<AtomicU64>`) in `Clone` (drop the custom clone), and add a test registering on two clones asserting both disposers unregister only their own listener?
**Answer:**

---

### BUG-004 (P1): session.cancel racing the prompt spawn loses the abort
**File(s):** `web-server/src/api.rs:741-750` (prompt spawns the turn task), `kernel-loop/src/lib.rs:324-327` (signal installed inside run_turn), `web-server/src/api.rs:861-873` (cancel calls abort())
**Severity:** High
**Observation:** `session.prompt` returns `{accepted:true}` and spawns the turn asynchronously; `ReactLoopAgent.abort()` only acts when `self.cancel` holds a signal, which happens mid-`run_turn` (after the UserMessage persist). A cancel arriving between the prompt response and the signal installation is silently dropped ("无活跃回合时无效果"), so the turn runs to completion despite an explicit client cancel — including the gap where `run_turn` is doing the UserMessage append/persist. With a slow/failing storage this window is real, and it is a plain race even on healthy systems.
**Question:** Move the signal installation to the point where `running` is set (i.e., let the agent expose `begin_turn(signal)` before spawning, or let `abort()` latch a "pending abort" the next `run_turn` consumes)?
**Answer:**

---

### BUG-005 (P1): Abort is not observed while the stream keeps producing — biased select starves the abort branch
**File(s):** `kernel-llm/src/openai.rs:595-607` (mid-stream `select! { biased; stream.next() => ..., wait_aborted => ... }`), also 486-498 for send
**Severity:** High
**Observation:** With `biased;`, `stream.next()` is polled first; while the provider delivers chunks continuously, the abort branch is never polled, so an abort set mid-stream takes effect only when the upstream stalls or hits EOF. For a fast/continuous stream the "cancel" keeps streaming chunks (each still logged + fsynced) until the stream ends — defeating the abort semantics claimed by the module docs and the adapter.spec mirror ("恰一个 aborted finish chunk"). The test only covers abort during a 5-second stall, not during active delivery.
**Question:** Check `signal.is_aborted()` at the top of each loop iteration (cheap AtomicBool) in addition to the select, and add a test where the server streams continuously while abort fires, asserting the stream terminates promptly with the single aborted finish?
**Answer:**

---

### BUG-006 (P2): Persist failure leaves appended-but-unpersisted events in the in-memory log
**File(s):** `kernel-loop/src/lib.rs:315-320` (UserMessage append→persist with `?`), 300-306 (persist helper)
**Severity:** Medium
**Observation:** Every `persist(&rec).await?` on failure returns `LoopError::Persist` **after** the event was already appended to the in-memory session (and emitted on the bus). The memory log then diverges from disk (violating logged-means-persisted), no `Turn Ended` is written, and a subsequent `run_turn(Some(text))` retry appends a **second** UserMessage — the first unpersisted one still contributes to `derive_messages` (model sees a duplicated message) and the bus listeners already forwarded it (client sees a user/message that is not on disk).
**Question:** Should persist failures roll back the in-memory append (or mark the session broken and refuse further turns until re-created/restored)?
**Answer:**

---

### BUG-007 (P2): Duplicate session.create replaces the live agent while disk state is unchanged
**File(s):** `web-server/src/api.rs:417-435` (client-supplied sessionId), `kernel-assembly/src/lib.rs:92-99`, `kernel-session/src/lib.rs:166-170`
**Severity:** Medium
**Observation:** `session.create` accepts a client-chosen `sessionId`. If it collides with an existing session, `SessionStore::create` silently replaces the in-memory session (new `Session` with a fresh SessionStarted on the bus) and then `persist.create_session` fails with "already exists". Result: the live table now points at a new agent whose log is not on disk, while the DB keeps the old log — subsequent prompts write to disk via the new agent's memory seq (starting at 1) colliding with existing DB rows (seq 1 already exists → append fails) or producing interleaved history. The old running turn (if any) keeps appending to the replaced session object, whose events still broadcast under the same session id.
**Question:** Reject duplicate creation before touching the store (check persist first, or make store.create return an error on duplicate)?
**Answer:**

---

### BUG-008 (P2): repair_interrupted_turn can leave earlier unpaired events behind
**File(s):** `kernel-assembly/src/lib.rs:185-225`
**Severity:** Medium
**Observation:** The repair cuts at the first unpaired Started scanning from the tail, but earlier anomalies survive. Example log `[Step Started, Turn Started]` (kill after Turn Started — possible once BUG-001 is fixed and Turn Started is emitted): scan finds Turn Started unpaired → cut at its index → truncation leaves the dangling `Step Started` — the "repaired" tail is still torn, and `verify_tail` would fail. The algorithm also counts `Step Ended` across turn boundaries without validating nesting.
**Question:** After fixing Turn Started emission, make repair iterate/truncate repeatedly (or validate the full pairing invariant post-truncation and refuse/repair until the tail is paired)?
**Answer:**

---

### BUG-009 (P2): resolve_thinking errors are silently dropped in build_request
**File(s):** `kernel-llm/src/openai.rs:355-362` (`if let Ok(Some((thinking, effort))) = Self::resolve_thinking(...)`)
**Severity:** Medium
**Observation:** When `resolve_thinking` returns `Err` (unknown reasoning effort → should be UNSUPPORTED_REASONING_EFFORT, or deployment-lock violation), the error is discarded and the request is sent **without any thinking config** — the provider silently applies its defaults instead of the caller being told the effort is unsupported. The rejection behavior is only unit-tested on `resolve_thinking` in isolation, never through `build_request`/`stream`.
**Question:** Propagate the error into the stream as `FinishReason::Error{code:"UNSUPPORTED_REASONING_EFFORT"}` (matching DSH serialize.spec)?
**Answer:**

---

### BUG-010 (P2): session.fork documented fallback for out-of-range atSeq is not implemented
**File(s):** `web-server/src/api.rs:608-610` (doc: "省略/越界回退最后完成 turn"), 636-648 (implementation)
**Severity:** Medium
**Observation:** For `atSeq` beyond the last turn/end, `turn_ends.iter().find(...)` returns `None` and the handler returns `fork-unavailable` — the documented "越界回退最后完成 turn" fallback does not exist. A client forking with a stale seq gets an error instead of the last completed turn.
**Question:** Implement the fallback (clamp to last turn end) or fix the doc/ledger to match the reject behavior?
**Answer:**

---

### BUG-011 (P3): Session::append can interleave seq order under concurrency
**File(s):** `kernel-session/src/lib.rs:103-109`
**Severity:** Low
**Observation:** `fetch_add` reserves the seq before `log.write().push`, so two concurrent appenders can push out of order (log vector order ≠ seq order), breaking the "按 seq 升序" invariant relied on by `from_log` validation and projections. Currently masked because web-server serializes turns per session (`running` flag), but `SessionStore::get` allows multiple agents/writers to share one session Arc.
**Question:** Serialize append under the log write lock (reserve seq inside the lock), or document a single-writer-per-session contract and enforce it?
**Answer:**

---

### BUG-012 (P3): session.history projections snapshot leaks other sessions' projections
**File(s):** `web-server/src/api.rs:178-184` (`projection_snapshot` iterates all keys), 506-511 (returned for any session)
**Severity:** Low
**Observation:** `projection_snapshot()` returns every projection unit regardless of session — `session.history` for session A includes goal projections written by session B. `write_projection` keys are global (`"goal"` per session id) with no namespace separation.
**Question:** Scope projections per session (key prefix or per-session map), or is a single shared projection space the intended contract?
**Answer:**

---

### BUG-013 (P3): Unknown block types silently degrade to Text in BlockAssembler
**File(s):** `kernel-loop/src/lib.rs:191-209` (assemble `_ => ContentBlock::Text`)
**Severity:** Low
**Observation:** A provider emitting a block type the assembler doesn't recognize (e.g., future "image" or a typo'd type) is silently converted into a text block carrying whatever delta text accumulated — the "绝不静默 flatten" philosophy applied in the OpenAI serializer is not applied here.
**Question:** Should unknown block types be dropped with a logged warning, or surfaced as an error finish?
**Answer:**

---

### BUG-014 (P3): Tool-result is_error flag lost in wire serialization; empty tool names execute
**File(s):** `kernel-llm/src/openai.rs:241-252` (is_error never serialized); `kernel-loop/src/lib.rs:535-567` (executes call with possibly empty name)
**Severity:** Low
**Observation:** The `'(no output)'` sentinel and output text are sent, but `isError` (present in the internal `ToolCallResult`) is not carried into OpenAI tool-result wire messages — the model cannot distinguish a failed tool call from a successful empty output. Separately, a tool-call block with missing name (assembler fallback `name: ""`) is executed against the registry and logged as an error result rather than rejected as malformed.
**Question:** Mirror DSH's tool-result message shape for errors (e.g., content prefix or a dedicated marker), and validate tool-call completeness before execution?
**Answer:**

---

## Improvements & Suggestions

### IMP-001 (P2): Add the two integration tests that would have caught BUG-001/BUG-002
**File(s):** `kernel-assembly/tests/`, `kernel-loop/src/lib.rs` tests
**Severity:** Medium
**Observation:** There is no end-to-end test asserting (a) turn numbers increment across two turns of one session, and (b) after restart+restore, live event seq continues from the persisted wire length. Both invariants are currently broken (BUG-001, BUG-002) and the existing `create_restore_roundtrip` test only asserts `steps >= 1`, not the turn number.
**Question:** Add these two tests as regression guards when fixing the bugs?
**Answer:**

---

### IMP-002 (P2): In-memory only state silently vanishes on restart
**File(s):** `web-server/src/api.rs:66-85` (settings, credentials, workspaces, goals, projections, attachments)
**Severity:** Medium
**Observation:** settings revisions, credentials (API keys!), workspaces, goals, archived ids and projections all live in process memory. A restart silently drops them — credentials revert to keyless (requests then fail MISSING_CREDENTIAL until re-set), settings revisions reset to 0 (clients holding revisions get conflicts), workspaces vanish. There is no warning at startup that state was lost.
**Question:** Persist these (at least credentials + settings + workspaces) to sqlite or the settings store, or explicitly document/log the volatility?
**Answer:**

---

### IMP-003 (P2): No read/idle timeout on LLM HTTP requests
**File(s):** `kernel-llm/src/openai.rs:91-94` (only connect_timeout=15s)
**Severity:** Medium
**Observation:** `stream_inner` has no overall request timeout or read idle timeout. A provider that accepts the connection and sends nothing (or stalls mid-stream without closing) hangs the turn indefinitely — no DSH-side timeout, and abort only helps if a client is attached to cancel. Combined with `running` flag, the session becomes permanently busy.
**Question:** Add a configurable request/read timeout (e.g., total 10 min, idle 60s) surfaced as a TRANSPORT/TIMEOUT finish?
**Answer:**

---

### IMP-004 (P3): WS broadcast lag silently drops events
**File(s):** `web-server/src/ws.rs:93` (mux `Err(_) => continue`), `ws.rs:216` (host)
**Severity:** Low
**Observation:** broadcast channel (capacity 256) `recv` returns `Lagged` for slow clients and the loops just `continue` — events vanish without any signal to the client. The frontend has no resync mechanism other than reconnecting, and reconnecting replays only the baseline (persisted events), which — combined with BUG-002 — can silently drop the intervening live events entirely.
**Question:** On `Lagged`, close the socket with a distinctive code or send a resync frame so the client re-attaches and re-reads history?
**Answer:**

---

### IMP-005 (P3): Supervisor edge cases: duplicate-spawn zombie and kill races
**File(s):** `kernel-supervisor/src/lib.rs:115-121` (duplicate spawn kills without wait task), 156-176 (kill `.take()`s completion)
**Severity:** Low
**Observation:** In the duplicate-id path the freshly spawned child is `start_kill()`ed and dropped with no wait task, so it is never reaped (zombie on Unix, potential leak on Windows) if it ignores the signal. Concurrent `kill()` calls race on `completion.take()` — the second caller skips the wait and can return `KillFailed` while the kill is in flight.
**Question:** Spawn a short-lived reaper task in the duplicate path, and make `kill` idempotent under concurrency?
**Answer:**

---

### IMP-006 (P3): Unbounded in-memory buffers for export and static serving
**File(s):** `web-server/src/lib.rs:335-355` (zip built fully in memory), `web-server/src/static_spa.rs:89` (`std::fs::read` whole file)
**Severity:** Low
**Observation:** session.export accumulates the entire session zip in a `Vec<u8>`, and the static handler reads whole files into memory. Fine for the local single-user profile, but large sessions/assets scale memory linearly with request size.
**Question:** Stream the zip via `Body::from_stream` (or cap export size), and stream file responses?
**Answer:**

---
