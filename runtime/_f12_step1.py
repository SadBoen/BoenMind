# 批7:F-12 依赖倒置(临时脚本,用完即删)
# 端口/DTO 收归 bm_core::ports::persist;bm-persist 反向依赖 + re-export 保旧路径。
import os, re

CORE = 'crates/bm-core'
PERS = 'crates/bm-persist'

# ---------- 1) bm-core: ports.rs → ports/ 目录 ----------
ports_rs = open(f'{CORE}/src/ports.rs', encoding='utf-8').read()
os.makedirs(f'{CORE}/src/ports', exist_ok=True)
open(f'{CORE}/src/ports/mod.rs', 'w', encoding='utf-8', newline='').write(
    ports_rs.rstrip('\n') + '\n\npub mod persist;\n')
os.remove(f'{CORE}/src/ports.rs')

# ---------- 2) 提取 bm-persist 侧类型/端口 ----------
err_src = open(f'{PERS}/src/error.rs', encoding='utf-8').read()
store_src = open(f'{PERS}/src/store.rs', encoding='utf-8').read()
recovery_src = open(f'{PERS}/src/recovery.rs', encoding='utf-8').read()
rows_src = open(f'{PERS}/src/sqlite_state/rows.rs', encoding='utf-8').read()
util_src = open(f'{PERS}/src/util.rs', encoding='utf-8').read()

# EventStore trait 全文(顶层项配平提取)
m = re.search(r'(?ms)^pub trait EventStore: Send \+ Sync \{.*?^\}$', store_src)
assert m, 'EventStore trait 未找到'
trait_body = m.group(0)
assert 'rusqlite' not in trait_body and 'StateDb' not in trait_body

# recovery.rs:类型区(RecoveryReport..WorldRows)与函数区分离
m = re.search(r'(?ms)^(/// ① 修复窗口.*?)$^pub fn repair_tail', recovery_src)
assert m, 'recovery 类型区未找到'
rec_types = m.group(1).rstrip()
rec_types = rec_types[:rec_types.rfind('}') + 1]  # WorldRows 收尾
# 函数区 = repair_tail 起到文件尾
m2 = re.search(r'(?ms)^/// ① 修复窗口.*$', recovery_src)
rec_fns = m2.group(0)

# rows.rs:四个行 DTO 全文(50 行,全搬)
row_dtos = rows_src.split('\n', 1)[1] if rows_src.startswith('//!') else rows_src

# util.rs:filter_lines_atomic 函数全文
m = re.search(r'(?ms)^pub fn filter_lines_atomic.*?^\}$', util_src)
assert m, 'filter_lines_atomic 未找到'
filter_fn = m.group(0)

# ---------- 3) 写 bm-core/ports/persist.rs ----------
persist_rs = (
    '//! 持久化端口层(F-12 依赖倒置):EventStore 端口与行 DTO 的**所有权**'
    '归内核 bm-core;bm-persist 作为实现方反向依赖内核,并 re-export 保持旧路径。\n'
    'use bm_contract::events::EventEnvelope;\n\n'
    '#[derive(Debug, thiserror::Error)]\n'
    'pub enum StoreError {\n'
    '    #[error("事件日志 IO 失败: {0}")]\n'
    '    Io(#[from] std::io::Error),\n'
    '    #[error("SQLite 失败: {0}")]\n'
    '    Sql(#[from] rusqlite::Error),\n'
    '    #[error("事件日志损坏于 seq {seq}: {reason}")]\n'
    '    Corrupt { seq: u64, reason: String },\n'
    '    #[error("CAS 不匹配: key={key} expect={expect}")]\n'
    '    CasMismatch { key: String, expect: String },\n'
    '    #[error("目录未初始化: {0}")]\n'
    '    NotOpen(String),\n'
    '}\n\n'
    'pub type StoreResult<T> = Result<T, StoreError>;\n\n'
    + row_dtos.replace('pub struct', '#[derive(Clone)]\npub struct', 1)
    + '\n' + rec_types + '\n\n'
    + trait_body + '\n\n'
    + filter_fn + '\n'
)
# rows DTO 需要 Clone derive——原 rows.rs 已有 derive?检查一下再定
open(f'{CORE}/src/ports/persist.rs', 'w', encoding='utf-8', newline='').write(persist_rs)
print('ports/persist.rs 写入', len(persist_rs), '字符')
print('rows.rs 原头部:', rows_src[:120])
print('recovery 类型区头:', rec_types[:80])
