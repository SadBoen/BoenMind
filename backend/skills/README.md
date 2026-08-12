# backend/skills —— BoenMind 自研 skill 分发源

与 `backend/plugins/` 同层级：此目录存放 BoenMind **官方自研 skill**（SKILL.md 目录），
作为仓库内分发源。安装方式见下；与 pi.dev / skills.sh 社区 skill 不同，这些 skill
不依赖网络目录，内容随仓库版本管理。

## 现状

| skill | 说明 | 来源 |
|---|---|---|
| `web-scraping/` | 网页采集工作流：站型四分类判定、web_fetch/curl 工具选型、验证纪律、铁律、站点笔记（CCS/MPA/JD/大陆工商源）、反检测基准结论 | 2026-08-12 自 Hermes（SadBoen/Hermes-Setting，Hermes-skill-web-scraping v2.0.0）迁移：吸收知识层，剔除 VPS 环境绑定，工具映射适配 BoenMind |

## 安装（本地路径安装）

```bash
curl -s -X POST http://127.0.0.1:17321/api/skills/install \
  -H "Content-Type: application/json" \
  -d '{"path": "/Users/boen/.zcode/workspace/BoenMind/backend/skills/web-scraping"}'
# 启用（新对话生效）
curl -s -X POST http://127.0.0.1:17321/api/skills/web-scraping \
  -H "Content-Type: application/json" -d '{"enabled": true}'
```

也可在设置 → Skill 页面的"本地安装"输入框中粘贴路径。

## 约定

- 目录名即 skill id（仅字母数字/`-`/`_`/`.`，≤64 字符）
- `SKILL.md` frontmatter 仅解析 `name` / `description`（description ≤200 字符，会注入
  agent 会话供模型匹配任务，触发词要写足）
- 相对路径引用以 skill 目录为基准解析（`references/...`）
- 修改后重新安装前需先卸载旧版本（`DELETE /api/skills/<id>`）
