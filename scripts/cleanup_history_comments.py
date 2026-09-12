#!/usr/bin/env python3
"""
清理代码与文档中的历史追溯性注释
策略：删除"此前/修复/时间戳/评审轮次/M批次"，保留核心约束与ADR引用
"""
import re
import sys
from pathlib import Path

# 历史标记模式（要删除的）
HISTORY_PATTERNS = [
    r'此前[^。；\n]*',  # "此前......"语句
    r'20\d{2}-\d{2}-\d{2}[^)）\n]*',  # 时间戳
    r'\(修复[^)）]*\)',  # (修复...)
    r'\(评审[^)）]*\)',  # (评审...)
    r'\(实测[^)）]*\)',  # (实测...)
    r'评审[轮批]次\d+',  # 评审轮次X
    r'M\d+\s*批次\d*',  # M7批次1
    r'第[一二三四五六七八九十\d]+轮评审',  # 第X轮评审
    r'回看\s*[：:][^。\n]*',  # 回看：...
    r'收官[^。\n]*',  # 收官...
]

def clean_line(line: str) -> str:
    """清理单行中的历史标记，仅处理注释行"""
    original = line.rstrip('\n\r')
    
    # 只处理注释行（Rust/JS/TS 的 // 或 Python 的 #）
    if not (re.match(r'^\s*//', original) or re.match(r'^\s*#', original)):
        return line  # 非注释行保持原样
    
    cleaned = original
    # 应用所有历史模式
    for pattern in HISTORY_PATTERNS:
        cleaned = re.sub(pattern, '', cleaned)
    
    # 清理多余空格
    cleaned = re.sub(r'\s+', ' ', cleaned)
    cleaned = cleaned.rstrip()
    
    # 如果注释变成空的，整行删除
    if re.match(r'^\s*//\s*$', cleaned) or re.match(r'^\s*#\s*$', cleaned):
        return ''
    
    # 如果清理后只剩标点，整行删除
    if re.match(r'^\s*[//\s#\-—:：,，。；]+\s*$', cleaned):
        return ''
    
    # 保留原始换行符
    return cleaned + '\n' if line.endswith('\n') else cleaned

def should_skip_file(filepath: Path) -> bool:
    """判断是否跳过文件"""
    skip_dirs = {'.git', 'node_modules', 'target', '.ignored', '.work', '.codegraph', '__pycache__'}
    return any(part in skip_dirs for part in filepath.parts)

def process_file(filepath: Path, dry_run: bool = True) -> tuple[int, int]:
    """处理单个文件，返回(原始行数, 删除行数)"""
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            lines = f.readlines()
    except Exception as e:
        print(f"跳过 {filepath}: {e}", file=sys.stderr)
        return 0, 0
    
    original_count = len(lines)
    cleaned_lines = []
    deleted_count = 0
    
    for line in lines:
        cleaned = clean_line(line)
        if cleaned == '' and line.strip() != '':
            deleted_count += 1
        else:
            cleaned_lines.append(cleaned if cleaned else line)
    
    if not dry_run and deleted_count > 0:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.writelines(cleaned_lines)
    
    return original_count, deleted_count

def main():
    import argparse
    parser = argparse.ArgumentParser(description='清理历史追溯性注释')
    parser.add_argument('--apply', action='store_true', help='实际执行清理（默认预览）')
    parser.add_argument('--filter', help='文件名过滤（如 bm-core）')
    args = parser.parse_args()
    
    root = Path(__file__).parent.parent
    patterns = ['**/*.rs', '**/*.py', '**/*.ts', '**/*.tsx', '**/*.md']
    
    total_files = 0
    total_deleted = 0
    
    for pattern in patterns:
        for filepath in root.glob(pattern):
            if should_skip_file(filepath):
                continue
            if args.filter and args.filter not in str(filepath):
                continue
            
            orig_count, del_count = process_file(filepath, dry_run=not args.apply)
            if del_count > 0:
                total_files += 1
                total_deleted += del_count
                print(f"{'[预览]' if not args.apply else '[修改]'} {filepath.relative_to(root)}: 删除 {del_count} 行")
    
    print(f"\n总计: {total_files} 个文件, {total_deleted} 行{'将被' if not args.apply else '已'}删除")
    if not args.apply:
        print("\n运行 --apply 实际执行清理")

if __name__ == '__main__':
    main()
