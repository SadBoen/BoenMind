# -*- coding: utf-8 -*-
"""Music App——本地音乐库与播放列表 App (ADR-0011; stdio MCP server).

提供本地音乐库扫描索引、搜索、播放列表维护等能力。
零外部依赖(Python 标准库)，通过标准 JSON-RPC 2.0 stdio MCP 协议与 Runtime 互联。
"""
import argparse
import hashlib
import json
import os
import re
import sys

AUDIO_EXTENSIONS = {".mp3", ".wav", ".ogg", ".flac", ".m4a", ".aac"}

# 进程内索引与播放列表
tracks_index = []  # list of track dicts: {id, title, artist, file_path, rel_path, size_bytes}
playlist = []      # list of track_id strings

TOOLS = [
    {
        "name": "music.scan",
        "description": "扫描音乐目录建立本地曲库索引",
        "inputSchema": {
            "type": "object",
            "properties": {
                "sub_dir": {"type": "string", "description": "音乐子目录(留空为根目录)"}
            },
        },
        "annotations": {},
    },
    {
        "name": "music.search",
        "description": "搜索曲库中的音乐(按标题或文件名模糊匹配)",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "搜索关键词"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
            },
            "required": ["query"],
        },
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "music.list_tracks",
        "description": "列出曲库中已索引的全部曲目",
        "inputSchema": {
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 100},
                "offset": {"type": "integer", "minimum": 0, "default": 0}
            }
        },
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "music.playlist_get",
        "description": "获取当前播放列表",
        "inputSchema": {"type": "object"},
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "music.playlist_add",
        "description": "向播放列表添加曲目",
        "inputSchema": {
            "type": "object",
            "properties": {
                "track_id": {"type": "string", "description": "曲目唯一标识"}
            },
            "required": ["track_id"],
        },
        "annotations": {},
    },
    {
        "name": "music.playlist_clear",
        "description": "清空播放列表",
        "inputSchema": {"type": "object"},
        "annotations": {},
    },
]


def scan_directory(base_dir, sub_dir=""):
    global tracks_index
    target_dir = os.path.normpath(os.path.join(base_dir, sub_dir)) if sub_dir else base_dir
    target_real = os.path.realpath(target_dir)
    base_real = os.path.realpath(base_dir)

    # 路径穿越防护
    if not (target_real == base_real or target_real.startswith(base_real + os.sep)):
        return {"error": "路径越界, 必须在音乐目录内"}

    if not os.path.isdir(target_real):
        return {"error": f"目录不存在: {sub_dir}"}

    scanned = []
    for root, _, files in os.walk(target_real):
        for f in sorted(files):
            ext = os.path.splitext(f)[1].lower()
            if ext in AUDIO_EXTENSIONS:
                full_path = os.path.join(root, f)
                rel_path = os.path.relpath(full_path, base_real).replace("\\", "/")
                # 简单解析歌名与艺术家 (如 "Artist - Title.mp3")
                stem = os.path.splitext(f)[0]
                if " - " in stem:
                    artist, title = stem.split(" - ", 1)
                else:
                    artist = "未知艺术家"
                    title = stem
                
                track_id = hashlib.sha256(rel_path.encode("utf-8")).hexdigest()[:16]
                size_bytes = os.path.getsize(full_path)
                
                scanned.append({
                    "id": track_id,
                    "title": title.strip(),
                    "artist": artist.strip(),
                    "rel_path": rel_path,
                    "size_bytes": size_bytes,
                    "format": ext.lstrip("."),
                })

    tracks_index = scanned
    return {"scanned_count": len(scanned), "root_dir": base_real}


def handle_tool_call(name, args, music_dir):
    global playlist
    if name == "music.scan":
        sub_dir = args.get("sub_dir", "")
        res = scan_directory(music_dir, sub_dir)
        if "error" in res:
            return {"isError": True, "content": [{"type": "text", "text": res["error"]}]}
        return {
            "content": [{"type": "text", "text": f"扫描完成, 共索引 {res['scanned_count']} 首曲目"}],
            "structuredContent": res,
        }

    if name == "music.search":
        q = args.get("query", "").lower()
        limit = args.get("limit", 20)
        matched = [
            t for t in tracks_index
            if q in t["title"].lower() or q in t["artist"].lower() or q in t["rel_path"].lower()
        ][:limit]
        return {
            "content": [{"type": "text", "text": json.dumps(matched, ensure_ascii=False, indent=2)}],
            "structuredContent": {"tracks": matched, "total": len(matched)},
        }

    if name == "music.list_tracks":
        limit = args.get("limit", 100)
        offset = args.get("offset", 0)
        sliced = tracks_index[offset:offset + limit]
        return {
            "content": [{"type": "text", "text": json.dumps(sliced, ensure_ascii=False, indent=2)}],
            "structuredContent": {
                "tracks": sliced,
                "total": len(tracks_index),
                "offset": offset,
                "limit": limit,
            },
        }

    if name == "music.playlist_get":
        tracks_map = {t["id"]: t for t in tracks_index}
        items = [tracks_map[tid] for tid in playlist if tid in tracks_map]
        return {
            "content": [{"type": "text", "text": json.dumps(items, ensure_ascii=False, indent=2)}],
            "structuredContent": {"playlist": items, "count": len(items)},
        }

    if name == "music.playlist_add":
        tid = args.get("track_id", "")
        tracks_map = {t["id"]: t for t in tracks_index}
        if tid not in tracks_map:
            return {"isError": True, "content": [{"type": "text", "text": f"未找到曲目 ID: {tid}"}]}
        playlist.append(tid)
        return {
            "content": [{"type": "text", "text": f"已添加「{tracks_map[tid]['title']}」至播放列表"}],
            "structuredContent": {"playlist_length": len(playlist), "added": tracks_map[tid]},
        }

    if name == "music.playlist_clear":
        playlist = []
        return {
            "content": [{"type": "text", "text": "播放列表已清空"}],
            "structuredContent": {"playlist_length": 0},
        }

    return {"isError": True, "content": [{"type": "text", "text": f"未知工具: {name}"}]}


def main():
    parser = argparse.ArgumentParser(description="BoenMind Music App MCP Server")
    parser.add_argument("--dir", default=os.getcwd(), help="音乐文件根目录")
    opts = parser.parse_args()

    music_dir = os.path.abspath(opts.dir)
    os.makedirs(music_dir, exist_ok=True)

    # 首次启动自动扫描根目录
    scan_directory(music_dir)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except Exception:
            continue

        method = req.get("method")
        msg_id = req.get("id")

        if method == "initialize":
            res = {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "music-player", "version": "0.1.0"},
                },
            }
            sys.stdout.write(json.dumps(res, ensure_ascii=False) + "\n")
            sys.stdout.flush()
        elif method == "notifications/initialized":
            continue
        elif method == "tools/list":
            res = {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {"tools": TOOLS},
            }
            sys.stdout.write(json.dumps(res, ensure_ascii=False) + "\n")
            sys.stdout.flush()
        elif method == "tools/call":
            params = req.get("params", {})
            name = params.get("name", "")
            args = params.get("arguments", {})
            result = handle_tool_call(name, args, music_dir)
            res = {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": result,
            }
            sys.stdout.write(json.dumps(res, ensure_ascii=False) + "\n")
            sys.stdout.flush()
        elif method in ("shutdown", "exit"):
            if msg_id is not None:
                sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg_id, "result": None}) + "\n")
                sys.stdout.flush()
            break


if __name__ == "__main__":
    main()
