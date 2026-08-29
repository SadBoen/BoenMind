#!/usr/bin/env python3
"""BoenMind 合同工件库校验器（README 中 CI 规则 R1-R4 的可执行形态）。

零依赖：内置 draft-07 子集校验器（覆盖本库使用的全部关键字：
type/enum/const/required/properties/additionalProperties/items/oneOf/
$ref(本地与跨文件 $id)/pattern/min·max/format:date-time）。

用法： python3 scripts/validate.py
退出码： 0 = 全部通过；1 = 存在失败。
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
problems = []


def fail(rule, msg):
    problems.append(f"[{rule}] {msg}")


# ---------- R1: 所有 JSON 可解析 ----------
docs = {}
for p in sorted(ROOT.rglob("*.json")):
    if p.parts[0] == "scripts":
        continue
    try:
        docs[p.relative_to(ROOT)] = json.loads(p.read_text(encoding="utf-8"))
    except Exception as e:  # noqa: BLE001
        fail("R1", f"{p.relative_to(ROOT)}: 非法 JSON: {e}")
print(f"R1  JSON 解析          : {len(docs)} 个文件，{sum(1 for x in problems if x.startswith('[R1'))} 个失败")

store = {}
for rel, d in docs.items():
    if isinstance(d, dict) and "$id" in d:
        store[d["$id"]] = d


# ---------- 内置 draft-07 子集校验器 ----------
def _frag(doc, frag):
    cur = doc
    for part in frag.strip("/").split("/") if frag else []:
        cur = cur[part.replace("~1", "/").replace("~0", "~")]
    return cur


def _type_ok(inst, t):
    if t == "object":
        return isinstance(inst, dict)
    if t == "array":
        return isinstance(inst, list)
    if t == "string":
        return isinstance(inst, str)
    if t == "integer":
        return isinstance(inst, int) and not isinstance(inst, bool)
    if t == "number":
        return isinstance(inst, (int, float)) and not isinstance(inst, bool)
    if t == "boolean":
        return isinstance(inst, bool)
    if t == "null":
        return inst is None
    return False


def validate(inst, schema, root, path="$", debug=False):
    """返回错误列表；debug=True 时返回 (错误列表, 命中的分支数)。"""
    errs = []

    if schema is True:
        return ([], 1) if debug else []
    if schema is False:
        return ([f"{path}: false schema"], 0) if debug else [f"{path}: false schema"]
    if not isinstance(schema, dict):
        return ([], 1) if debug else []

    if "$ref" in schema:
        ref = schema["$ref"]
        if ref.startswith("#/"):
            target, new_root = _frag(root, ref[1:]), root
        else:
            base, _, frag = ref.partition("#")
            if base not in store:
                return ([f"{path}: 未注册的 $id '{base}'"], 0) if debug else [f"{path}: 未注册的 $id '{base}'"]
            target, new_root = _frag(store[base], frag), store[base]
        r = validate(inst, target, new_root, path, debug)
        return r if debug else r

    hits = 0
    if "const" in schema:
        if inst == schema["const"]:
            hits += 1
        else:
            errs.append(f"{path}: const 应为 {schema['const']!r}")
    if "enum" in schema:
        if inst in schema["enum"]:
            hits += 1
        else:
            errs.append(f"{path}: {inst!r} 不在枚举 {schema['enum']}")
    if "type" in schema:
        ts = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
        if any(_type_ok(inst, t) for t in ts):
            hits += 1
        else:
            errs.append(f"{path}: 类型应为 {ts}，实际 {type(inst).__name__}")
    if "oneOf" in schema:
        ok_n = sum(1 for s in schema["oneOf"] if not validate(inst, s, root, path))
        if ok_n == 1:
            hits += 1
        else:
            errs.append(f"{path}: oneOf 命中 {ok_n} 个分支（应恰为 1）")

    if isinstance(inst, str):
        if "minLength" in schema and len(inst) < schema["minLength"]:
            errs.append(f"{path}: 长度 < {schema['minLength']}")
        if "maxLength" in schema and len(inst) > schema["maxLength"]:
            errs.append(f"{path}: 长度 > {schema['maxLength']}")
        if "pattern" in schema and not re.search(schema["pattern"], inst):
            errs.append(f"{path}: 不匹配 pattern {schema['pattern']}")
        if schema.get("format") == "date-time" and inst is not None:
            if not isinstance(inst, str) or not re.fullmatch(
                r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z", inst
            ):
                errs.append(f"{path}: 非法 date-time（要求 ISO-8601 UTC）: {inst!r}")
    if isinstance(inst, (int, float)) and not isinstance(inst, bool):
        if "minimum" in schema and inst < schema["minimum"]:
            errs.append(f"{path}: < minimum {schema['minimum']}")
        if "maximum" in schema and inst > schema["maximum"]:
            errs.append(f"{path}: > maximum {schema['maximum']}")
    if isinstance(inst, list):
        if "minItems" in schema and len(inst) < schema["minItems"]:
            errs.append(f"{path}: 元素数 < {schema['minItems']}")
        if "maxItems" in schema and len(inst) > schema["maxItems"]:
            errs.append(f"{path}: 元素数 > {schema['maxItems']}")
        if "items" in schema:
            for i, item in enumerate(inst):
                errs += validate(item, schema["items"], root, f"{path}/{i}")
    if isinstance(inst, dict):
        for k in schema.get("required", []):
            if k not in inst:
                errs.append(f"{path}: 缺必填字段 '{k}'")
        props = schema.get("properties", {})
        for k, v in inst.items():
            if k in props:
                errs += validate(v, props[k], root, f"{path}/{k}")
            else:
                ap = schema.get("additionalProperties", True)
                if ap is False:
                    errs.append(f"{path}: 出现未定义字段 '{k}'")
                elif isinstance(ap, dict):
                    errs += validate(v, ap, root, f"{path}/{k}")
    if debug:
        return (errs, 1 if hits and not errs else 0)
    return errs


# ---------- R3/R4/R2: 每条黄金轨迹逐文件校验(遍历 golden-traces/*.md) ----------
etypes = {e["type"] for e in docs[Path("registry/runtime-events.v0_1.json")]["events"]}
codes = {c["code"] for c in docs[Path("registry/error-codes.v0_1.json")]["codes"]}
machines = docs[Path("state-machines/core-transitions.v0_1.json")]["machines"]
edges = {m: {(t["from"], t["to"]) for t in spec["transitions"]} for m, spec in machines.items()}
ENV = store["boenmind:wire:envelope:v0.1"]
AGENT = store["boenmind:wire:agent:v0.1"]
LOGS = store["boenmind:logs:execution-log-entry:v0.1"]

trace_files = sorted((ROOT / "golden-traces").glob("*.md"))
total_kinds = {"request": 0, "response": 0, "event": 0, "log": 0, "receipt": 0}
total_trans = 0
total_events = set()
total_codes = set()
for tf in trace_files:
    trace = tf.read_text(encoding="utf-8")

    # R3: 事件类型与错误码必须在注册表内
    used_events = set(re.findall(r'"type":\s*"([a-z]+(?:\.[a-z_]+)+)"', trace))
    used_events |= set(re.findall(r"事件\s*\d+[' ]*\s+([a-z]+(?:\.[a-z_]+)+)", trace))
    used_codes = set(re.findall(r'"(?:error_)?code":\s*"([a-z_]+)"', trace))
    used_codes |= set(re.findall(r'(?<![a-z_])(?:error_)?code:\s*"([a-z_]+)"', trace))
    total_events |= used_events
    total_codes |= used_codes
    for t in sorted(used_events - etypes):
        fail("R3", f"{tf.name}: 轨迹事件类型不在注册表: {t}")
    for c in sorted(used_codes - codes):
        fail("R3", f"{tf.name}: 轨迹错误码不在注册表: {c}")

    # R4: 状态迁移必须是迁移表中的边
    for machine, chain in re.findall(r"\b(operation|agent|session)\s+([a-z_]+(?:→[a-z_]+)+)", trace):
        states = chain.split("→")
        for a, b in zip(states, states[1:]):
            total_trans += 1
            if (a, b) not in edges[machine]:
                fail("R4", f"{tf.name}: {machine}: {a}→{b} 不是迁移表中的合法边")

    # R2: payload 必须通过对应 schema
    kinds = {"request": 0, "response": 0, "event": 0, "log": 0, "receipt": 0}
    for bi, raw in enumerate(re.findall(r"```json\n(.*?)```", trace, re.S), 1):
        label = f"{tf.name} 第{bi}个JSON块 {raw.strip()[:48]!r}…"
        try:
            obj = json.loads(raw)
        except Exception as e:  # noqa: BLE001
            fail("R2", f"{label}: 不可解析: {e}")
            continue
        errs = []
        if "method" in obj:
            errs += validate(obj, ENV["request"], ENV)
            kinds["request"] += 1
        elif "ok" in obj:
            errs += validate(obj, ENV["response"], ENV)
            kinds["response"] += 1
        if isinstance(obj, dict) and isinstance(obj.get("result"), dict) and "task_type" in obj["result"]:
            errs += validate(obj["result"], _frag(AGENT, "definitions/receipt"), AGENT)
            kinds["receipt"] += 1
        if isinstance(obj, dict) and "event_seq" in obj:
            errs += validate(obj, ENV["event_envelope"], ENV)
            kinds["event"] += 1
        if isinstance(obj, dict) and "log_seq" in obj:
            errs += validate(obj, LOGS, LOGS)
            kinds["log"] += 1
        for e in errs:
            fail("R2", f"{label}: {e}")
    for k, n in kinds.items():
        total_kinds[k] += n

print(f"R3  注册表覆盖         : 轨迹 {len(trace_files)} 条，事件 {len(total_events)} 种 / "
      f"错误码 {len(total_codes)} 种，{sum(1 for x in problems if x.startswith('[R3'))} 个越界")
print(f"R4  状态迁移边检查     : 轨迹中 {total_trans} 次迁移，"
      f"{sum(1 for x in problems if x.startswith('[R4'))} 个非法")
print(f"R2  payload 校验       : 校验 {sum(total_kinds.values())} 个负载 "
      f"(request={total_kinds['request']} response={total_kinds['response']} "
      f"event={total_kinds['event']} log={total_kinds['log']} receipt={total_kinds['receipt']}），"
      f"{sum(1 for x in problems if x.startswith('[R2'))} 个失败")

# ---------- 结果 ----------
print("-" * 56)
if problems:
    print(f"失败 {len(problems)} 项：")
    for x in problems:
        print("  " + x)
    sys.exit(1)
print("全部通过 ✓")
