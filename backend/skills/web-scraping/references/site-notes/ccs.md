# CCS 中国船级社（ccs.org.cn）— curl 直抓即可

**难度 1/5（除偶发 502）。服务端渲染，不需要浏览器引擎。**

## 核心事实

- **502 Bad Gateway 是服务器偶发**（Tengine，多次复测 1/3~1/5 概率），与 UA/指纹无关，**重试即可**（指数退避，最多 5 次）
- **必须带完整浏览器 UA**，裸/短 UA 必 502
- **正文页是服务端渲染，curl 直抓即可**：`ccswz/articleDetail?id=<17位id>`（带不带 columnId 参数均可）返回 70KB 完整 HTML，标题、摘要、正文段落、上一篇/下一篇全在
- ⚠️ **不要被误导**：页面里的 `id="fk_content"` 是底部"意见反馈表单"的输入框，不是正文容器；
  页面提示"正在查询，请稍候"属表单场景，不要据此判断正文异步加载——实测整页正文已在 HTML 中

## 页面结构

- 正文标题：`<h3 class="fnt_36">`
- 发布时间：`发布时间：<span>`
- 正文容器：`div.deta_con`（**实际 class 是 `deta_con wrap cf mt_15 mb_30`，匹配需前缀**；首段 `<div id="intro">` 是摘要，且摘要与第一段正文重复，抽取后需去重），正文结束于 `<div class="pager-close"`
- 图片：`ccswzimg.upload/cn/article/pic/`

## 列表页

- `ccswz/articleList?columnid=<id>&p=N`，每页 6 条（`p=` 与 `currentPage=` 参数均有效）
- 条目格式：`<h3><a href="articleDetail?id=...">标题</a></h3>` + `<p class="fnt_16">日期</p>`
- 总页数在 `gotoPageCount` value；页内另有"共3116条"字样可核对总量
- 栏目 id：新闻 = **201900002000000096**（446 页 ≈ 3116 条）、党建 = 202103030114000960

## 采集速率参考

- 列表 ~6 页/秒（每页 curl ~1s + 0.5s sleep），正文 ~0.7 篇/秒
- 3116 条全量约 1.5 小时，**但除非用户要数据本身，别跑全量**（见 SKILL.md 铁律 1）
- 脚本写法参考：列表+正文两级、502 指数退避重试、断点续爬（jsonl）；需要时按本笔记重建
