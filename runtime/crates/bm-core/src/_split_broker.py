# 批1:broker.rs → broker/ 目录模块(临时脚本,用完即删)
import os

SRC = 'crates/bm-core/src/broker.rs'
DST = 'crates/bm-core/src/broker'
os.makedirs(DST + '/tests', exist_ok=True)

lines = open(SRC, encoding='utf-8').read().splitlines(keepends=True)

def seg(a, b):  # 1-indexed inclusive
    return ''.join(lines[a-1:b])

# 校验边界锚点
assert '不依赖消费退还。' in lines[12], lines[12]
assert lines[28].startswith('/// 调用上下文'), lines[28]
assert lines[191].strip() == '}', lines[191]
assert 'LedgerEntry {' in lines[193], lines[193]
assert lines[366].strip() == '}', lines[366]
assert 'pub struct Broker' in lines[371], lines[371]
assert 'fn resource_matches' in lines[733], lines[733]
assert lines[765].strip() == '}', lines[765]
assert lines[765+0] is not None

header_doc = seg(1, 13)
imports = seg(19, 27)
types = seg(29, 192)
ledger = seg(194, 367)
core = seg(369, 733)
predicates = seg(734, 765)

# ---- types.rs ----
open(DST + '/types.rs', 'w', encoding='utf-8', newline='').write(
    '//! Broker 决策与执行的公共类型(自 broker.rs 机械移入;条目与行序原样)。\n'
    'use bm_contract::capability::{CapabilityManifest, DataTrust};\n'
    'use crate::registry::CapabilityProvider;\n'
    'use std::sync::Arc;\n\n'
    + types)

# ---- ledger.rs ----
open(DST + '/ledger.rs', 'w', encoding='utf-8', newline='').write(
    '//! Grant 台账(M4 内存版;「调用方×目标 Capability」O(1) 查表;自 broker.rs 机械移入)。\n'
    'use bm_contract::capability::{Grant, GrantScope};\n'
    'use bm_contract::timestamp::parse_ts;\n'
    'use std::collections::HashMap;\n'
    'use super::types::LedgerError;\n\n'
    + ledger)

# ---- predicate.rs ----
open(DST + '/predicate.rs', 'w', encoding='utf-8', newline='').write(
    '//! 资源谓词与 Provider 装配辅助(自 broker.rs 机械移入)。\n'
    'use bm_contract::capability::GrantResource;\n'
    'use std::sync::Arc;\n\n'
    + predicates.replace('fn resource_matches', 'pub(super) fn resource_matches', 1)
                .replace('fn json_scalar_eq', 'fn json_scalar_eq', 1))

# ---- mod.rs ----
mod_rs = (header_doc + '\n' + imports + '\n'
    + 'mod ledger;\nmod predicate;\nmod types;\n\n'
    + 'pub use ledger::GrantLedger;\n'
    + 'pub use predicate::provider_fn;\n'
    + 'pub use types::{CallContext, CallCredential, CallOutcome, Decision, DenyReason, Lease, LeaseError, PreparedCall, TrustViolation};\n\n'
    + core + '\n'
    + '#[cfg(test)]\nmod tests;\n'
    + '#[cfg(test)]\nmod trust_gate_tests;\n'
    + '#[cfg(test)]\nmod m7_tests;\n')
open(DST + '/mod.rs', 'w', encoding='utf-8', newline='').write(mod_rs)

# ---- 测试三件(抽 mod 壳,保留体内;use super::* → use crate::broker::*;) ----
tests = seg(766, 1663)
i_tests = tests.find('mod tests {')
i_trust = tests.find('mod trust_gate_tests {')
i_m7 = tests.find('mod m7_tests {')
assert i_tests == 0 and 0 < i_trust < i_m7
unit_body = tests[i_tests:i_trust]
trust_body = tests[i_trust:i_m7]
m7_body = tests[i_m7:]

def strip_mod(body):
    # 去掉外层 'mod xxx {' 与配对收尾 '}'(body 以 mod 开头、以单个 '}' 结尾)
    inner = body[body.find('{') + 1:]
    assert inner.rstrip().endswith('}')
    inner = inner.rstrip()[:-1]
    # 去一级缩进(每行去掉 4 空格前缀)
    out = []
    for l in inner.splitlines(keepends=True):
        out.append(l[4:] if l.startswith('    ') else l)
    return ''.join(out).replace('use super::*;', 'use crate::broker::*;', 1)

open(DST + '/tests/unit.rs', 'w', encoding='utf-8', newline='').write(
    '//! Broker 单元测试(自 broker.rs 机械移入)。\n' + strip_mod(unit_body))
open(DST + '/tests/trust_gate.rs', 'w', encoding='utf-8', newline='').write(
    '//! 信任门测试(自 broker.rs 机械移入)。\n' + strip_mod(trust_body))
open(DST + '/tests/m7.rs', 'w', encoding='utf-8', newline='').write(
    '//! M7 熔断/租约/抽屉测试(自 broker.rs 机械移入)。\n' + strip_mod(m7_body))
open(DST + '/tests/mod.rs', 'w', encoding='utf-8', newline='').write(
    '//! broker 测试三件(原 broker.rs 内嵌模块,机械移入;内容零改动)。\n'
    'mod m7;\nmod trust_gate;\nmod unit;\n')

os.remove(SRC)
print('broker 拆分完成:', os.listdir(DST), os.listdir(DST + '/tests'))
