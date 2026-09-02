# 音乐播放器 App 实现规格 (MUSIC-APP)

日期: 2026-09-03 · 来源: 用户 2026-09-02 裁决新工批次 · 状态: 实现中

## 1. 定位与架构设计

根据基线 §4 与 ADR-0011(首批真实 App 以 MCP Server 形态接入)，音乐播放器 App（简单版）遵循标准 App 形态落地：
- **后端面**: `apps/music_server.py` —— 独立进程运行的标准 stdio MCP server (Python 零第三方依赖标准库)，通过 `--dir <音乐目录>` 管理音频库与播放列表。
- **能力面 (Capabilities)**:
  - `music.scan`: 扫描指定目录内的音频文件（.mp3 / .wav / .ogg / .flac / .m4a / .aac），构建本地曲库索引；
  - `music.search`: 按歌名/文件名/艺术家搜索音乐曲目；
  - `music.list_tracks`: 列出当前曲库索引中的所有音频；
  - `music.playlist_get`: 获取当前播放列表与播放队列；
  - `music.playlist_add`: 向播放列表添加曲目；
  - `music.playlist_clear`: 清空播放列表。
- **前端面 (UI)**:
  - 工作区标签页扩展「音乐」页签 (`WorkspacePanel` 的 `music` tab) 与 Rail 音乐快速入口；
  - 网页内集成 HTML5 Audio 播放器控件（播放/暂停、上一曲/下一曲、进度条拖拽、音量调节、列表循环）；
  - 界面提供曲目搜索、一键扫描、播放列表管理与曲目点击即播。

## 2. 合同与安全性

- **隔离性**: 仅在指定的音乐目录或工作区路径内扫描与索引，禁止任意系统路径穿越（`os.path.realpath` 防逃逸）；
- **只读与副作用标注**:
  - `music.scan` / `music.playlist_add` / `music.playlist_clear`: 可逆操作 / 状态维护；
  - `music.search` / `music.list_tracks` / `music.playlist_get`: `readOnlyHint: true`（只读直通）；
- **配置接入**: 在 `apps/mcp-config.example.json` 中提供标准 MCP 安装配置。

## 3. 验收门

1. `apps/music_server.py` 支持标准 MCP 协议握手、`tools/list` 与 `tools/call`，包含单元测试与 mock 音频索引测试；
2. 前端工作区呈现「音乐」页签，具备完整播放器控制栏与曲目列表；
3. 真实浏览器手测验证：界面渲染正常、曲目列表展示清晰、控制按钮交互响应正常、截图留存。
