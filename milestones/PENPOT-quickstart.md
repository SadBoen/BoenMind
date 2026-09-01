# Penpot 大白话速成(给用户的"共同画板"用法)

> 2026-08-30 随 web 改版交付。用途:说不清想要的界面效果时,在 Penpot 里拖个
> 示意给我看;平时不必用,"我写你看截图"的循环够用。
>
> ⚠ 过时标注(2026-09-01):本文「备查」节提到的 runtime/web/tokens.css 令牌对接
> 方案已随 dsh 前端废止失效(现前端=runtime/webapp,主题令牌在 w3/theme.css);
> 画板沟通用法本身仍有效。

## 它是什么

开源版 Figma(画界面稿的工具),浏览器里用,不用装任何东西。

## 怎么开始(三步)

1. 打开 https://penpot.app → 注册账号(免费,界面可选中文);
2. 新建文件(Dashboard)→ 左侧"资源库/Assets"里拖任意组件到画布
   (按钮、卡片、输入框都有现成的);
3. 点中组件 → 右侧面板改颜色/大小 → 摆出你想要的样子。

你只需要三个动作:**拖、改、标**(哪里说不清画个箭头写两个字)。
图层/约束/组件变体等深水区一概不用碰。

## 画完之后

把文件设为公开分享(或截图)发我,我照稿实现。稿子只是示意——
比例、字号不用精确,位置和意思对就行。

## 备查(将来可选,现在不用做)

- 若日后想"自己动手调 BoenMind 外观",可以自托管一个 Penpot
  (Docker Compose,到时我来装):它的 Design Tokens 可与 BoenMind 的
  `runtime/web/tokens.css` 令牌表对接,可视化改色改字导出回填。
- 现成开源组件库(Essential UI Kit、Ant Design kit)可当参考词汇:
  https://penpot.app/penpothub/libraries-templates
