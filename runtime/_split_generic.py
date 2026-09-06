# 批3/4 通用拆分器:按顶层项(花括号配平)提取,分桶写文件(临时脚本,用完即删)
# 用法:python _split_generic.py <源文件相对路径> <目标目录> <桶映射 JSON>
# 桶映射:{"文件名": ["项名前缀匹配列表"], ...}; "*" = 默认桶(mod.rs)
import os, re, sys, json

SRC = sys.argv[1]
DST = sys.argv[2]
BUCKETS = json.loads(sys.argv[3])

lines = open(SRC, encoding='utf-8').read().splitlines(keepends=True)

# ---- 扫描顶层项:列 0 开始的项头行,花括号配平到收尾 ----
items = []  # (name, start_line, end_line) 1-indexed inclusive
i = 0
n = len(lines)
depth = 0
cur_start = None
cur_name = None
in_block_comment = False
while i < n:
    line = lines[i]
    if depth == 0:
        s = line.strip()
        # 跳过空行/注释(顶层)
        if (not s or s.startswith('//') or (s.startswith('/*') and '*/' in s)
                or s.startswith('#[') or s.startswith('//!') or s.startswith('///')):
            i += 1
            continue
        # 项头
        m = re.match(
            r'(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:fn|struct|enum|impl|mod|const|static|type|use)\b',
            line)
        if m:
            cur_start = i + 1
            if line.lstrip().startswith('use '):
                j = i
                while ';' not in lines[j]:
                    j += 1
                items.append(('use' + str(cur_start), 'use', cur_start, j + 1))
                i = j + 1
                continue
            kind_m = re.search(r'(fn|struct|enum|impl|mod|const|static|type)\b', line)
            kind = kind_m.group(1)
            nm = re.search(kind + r'\s+([A-Za-z0-9_]+)', line)
            cur_name = (nm.group(1) if nm else 'impl_' + kind + str(cur_start))
            if kind == 'use':
                # use 行到分号为止
                j = i
                while ';' not in lines[j]:
                    j += 1
                items.append((cur_name, 'use', cur_start, j + 1))
                i = j + 1
                continue
            # 多行签名:先推进到首个含 '{' 的行
            j = i
            depth = 0
            while True:
                depth += lines[j].count('{') - lines[j].count('}')
                if depth > 0 or ('{' in lines[j] and j > i):
                    break
                j += 1
            # 单行项(type/const 等)
            while depth > 0:
                j += 1
                l2 = lines[j]
                # 剥行注释与字符串再数括号(粗粒度,足够 Rust 顶层)
                l2 = re.sub(r'"(\\.|[^"\\])*"', '""', l2)
                l2 = re.sub(r'//.*', '', l2)
                depth += l2.count('{') - l2.count('}')
            items.append((cur_name, kind, cur_start, j + 1))
            i = j + 1
            continue
        # 其他顶层散行(属性独立行已跳过;遇到就并入上一项/忽略)
        i += 1
        continue
    else:
        i += 1

print('顶层项数:', len(items))
for it in items:
    print(' ', it[0], it[1], it[2], '-', it[3])

def bucket_of(name, kind):
    if kind == 'use':
        return '__header__'
    for fname, patterns in BUCKETS.items():
        for pat in patterns:
            if name.startswith(pat):
                return fname
    return '__mod__'

os.makedirs(DST, exist_ok=True)
header = ''  # 原 use 段与文件头 doc
# 文件头 = 起始连续 //! 注释 + use 行
hdr_end = 0
for idx, l in enumerate(lines):
    if l.strip().startswith('//!') or l.strip().startswith('use ') or l.strip().startswith('pub use '):
        hdr_end = idx + 1
    elif l.strip() == '':
        continue
    else:
        break
header = ''.join(lines[:hdr_end])

body_of = {}
for (name, kind, a, b) in items:
    bkt = bucket_of(name, kind)
    body_of.setdefault(bkt, []).append((name, kind, a, b))

# 各文件写入
used_buckets = set(body_of.keys())
decls = []
for fname in BUCKETS:
    if fname not in body_of:
        continue
    used_buckets.add(fname)
    is_mod = fname == '__mod__'
    fn = 'mod.rs' if is_mod else fname + '.rs'
    parts = []
    if not is_mod:
        parts.append('//! 自 %s 机械移入(内容零改动)。\n' % os.path.basename(SRC))
        parts.append('use super::*;\n\n')
    for (name, kind, a, b) in body_of[fname]:
        seg = ''.join(lines[a-1:b])
        parts.append(seg if seg.endswith('\n') else seg + '\n')
    open(os.path.join(DST, fn), 'w', encoding='utf-8', newline='').write(''.join(parts))
    if not is_mod:
        decls.append('mod %s;' % fname)

# mod.rs:声明 + 各桶 re-export(pub(super) 便于兄弟/父路径可见)
mod_parts = [header, '\n']
for fname in BUCKETS:
    if fname == '__mod__' or fname not in body_of:
        continue
    mod_parts.append('mod %s;\n' % fname)
mod_parts.append('\n')
for fname, its in body_of.items():
    if fname == '__mod__':
        continue
    names = ', '.join(n for (n, k, a, b) in its if k != 'use')
    if names:
        mod_parts.append('pub(super) use %s::{%s};\n' % (fname, names))
mod_parts.append('\n')
for (name, kind, a, b) in body_of.get('__mod__', []):
    seg = ''.join(lines[a-1:b])
    mod_parts.append(seg if seg.endswith('\n') else seg + '\n')
open(os.path.join(DST, 'mod.rs'), 'w', encoding='utf-8', newline='').write(''.join(mod_parts))

os.remove(SRC)
print('拆分完成 →', DST)
