---
name: web-scraping
description: 网页采集/抓取/爬虫必查（2026-08 自 Hermes 迁移）：站型四分类判定、web_fetch/curl 工具选型、JS 渲染与 CF 反爬处理、验证纪律、站点笔记（CCS/MPA/JD/大陆工商源）。用户说"采集X网站"或"从X网站找数据"时先加载本 skill。
---

# 网页采集中心（Web Scraping Center）

> 2026-08-12 自 Hermes（Hermes-skill-web-scraping v2.0.0）迁移：吸收其知识层（站型判定
> 工作流、验证纪律、铁律、站点笔记、反检测基准结论）；工具映射已适配 BoenMind 工具面
> （web_search / web_fetch / bash-curl），环境绑定信息（VPS 路径、systemd、引擎命令）已剔除。
> 本 skill 是**采集任务的唯一入口**：接到"采集 XXX / 从 XXX 网站找数据"先读完本节再动手。

## 首要工作流（顺序执行，不要跳）

```
1. 判定目标站是什么类型？——先跑 curl 基线（见下），看返回：
   - 200 + 数据节点齐全 → 普通站（服务端渲染）→ web_fetch 或 curl，完事
   - 200 + 空壳（JS 渲染）→ 评估值不值得，再决定（见"能力边界"）
   - CF 挑战页（just a moment / verify you are human）→ 同上
   - 登录墙/风控 → 先评估值不值得，再决定（参考 references/site-notes/jd.md）
2. 查 references/site-notes/ 对应站点笔记 —— 已踩过的坑直接看结论，别重踩
3. 工具选型：按下方映射表从低成本到高成本
4. 验证成功标准：数真实数据节点/正文关键词，不信 exit code 或字节数
5. 采集是手段不是目的：用户要的是数据/结论，别沉迷全量抓取（铁律 1）
```

## 工具选型（BoenMind 工具面，2026-08-12 适配）

| 场景 | 用谁 | 说明 |
|---|---|---|
| 服务端渲染的普通站 | **web_fetch**（首选） | Firecrawl 优先 + Jina Reader 兜底；免费额度有限，正文截断 8K 摘要 |
| 批量/深爬（数十页以上） | **curl**（bash 工具） | 免费不限量；先 probe 单页确认结构再写循环脚本 |
| 搜索定位 URL | **web_search** | 多源聚合（quick 档免费源 / deep 档含 Serper），结果带 `[源]` 标注 |
| 基线探测（第一步必做） | **curl**（bash 工具） | macOS 自带；必须带完整浏览器 UA；看 HTTP 码 + 数数据节点 |
| JS 渲染 / CF 反爬站 | **无内置引擎** | 不硬刚，先评估值不值得（见"能力边界"） |
| 登录墙 / 风控 | 先评估 | 判定标准见 references/site-notes/jd.md |

### curl 基线（诊断第一步，永远先做）

```bash
UA="Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36"
curl -sS -L --max-time 15 -A "$UA" -w "\nHTTP %{http_code}\n" "URL" -o /tmp/probe.html
grep -c '<数据节点特征>' /tmp/probe.html   # 数真实数据节点，判断是不是 JS 空壳
```

三种典型失败模式：

- **200 但无数据节点**（HTML 几十 KB、内容类节点 0 个）→ JS 渲染壳 → 见"能力边界"
- **重定向到风控页**（如京东 risk_handler、标题含"验证"）→ 反爬拦截 → 评估值不值得
- **502/403/登录墙** → 先排除服务器偶发（CCS 的 502 是 1/3~1/5 概率，重试即可），再考虑其他

## 能力边界（诚实声明，2026-08-12）

BoenMind 环境事实：插件沙箱无 exec、无 Python 生态、无浏览器引擎；agent 有 bash 工具与
web_search / web_fetch。因此：

- **JS 渲染站 / CF 反爬站当前抓不了**（无 Obscura/Camoufox 类引擎，也不应安装重型工具链——铁律 3）
- 遇到此类目标：先按 site-notes 判定值不值得 → 值得则让用户浏览器打开/代取，或评估外部抓取服务
- **IP 权重 > 引擎权重**（反检测基准结论）：桌面机出口是住宅/办公 IP，比数据中心 IP 友好得多——
  VPS 上"无解"的站（京东、大陆工商源）在桌面端可能直接可抓，值得重测并更新站点笔记

## 铁律（用户明确纠正过，迁移保留）

1. **采集是测试工具的手段，不是交付物**：目标站验证到"能抓到真实数据+解析正常"即可止步，
   不要主动启动全量采集（曾跑偏：CCS 3116 条全量被叫停）
2. **要通用工具，不要专用脚本**：交付可复用能力（输入 URL → 拿内容），单站点专用解析脚本只是应用示例
3. **禁止静默安装重型工具链**：装编译器/浏览器/引擎前必须列清单+回滚方案，征得同意再装
4. **用户顺口说的业务背景只是解释为什么需要数据，不等于授权转向领域调研**，做之前先问

## 站点笔记索引（细节在 references/site-notes/）

| 站点 | 结论一句话 |
|---|---|
| **京东** | 高难：登录态+大陆 IP 双关；VPS 已判放弃，桌面端待重测；实时价格无免费路径 |
| **CCS 中国船级社** | 难度 1/5：curl 直抓即可，正文服务端渲染；502 是服务器偶发重试即可（必须完整 UA） |
| **新加坡 MPA** | 难度 1/5：curl 直抓即可；通告类数据一条 RSS 全量拿（/feeds/media-releases）；SRS 船舶注册信息在 register-with-srs/ 子页 |
| **大陆工商源** | VPS 结论：天眼查/企查查/公示系统地域封锁不可达；桌面端（大陆网络）可直接访问，重测后更新笔记 |

*(新站点)*：抓完把经验追加到 references/site-notes/ —— 每个新站都值得记。

## 支持文件

- `references/antidetect-benchmark.md` — 2026 反检测基准（外部 31 目标 + 原 VPS 本机 6×7）结论层：判定词、IP 权重、引擎事实卡
- `references/site-notes/ccs.md` / `mpa.md` / `jd.md` / `cn-company-lookup.md` — 站点实测笔记
