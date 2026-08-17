# BoenMind（dsh 内核版）

以 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) 为核心的 BoenMind 扶正项目——dsh 是运行时真身，BoenMind 的差异化能力（专家团队/管家/皮肤/审计/记忆）全部以 dsh 插件形态叠加。

> 状态：M0 全家桶 bootstrap（2026-08-17 启动）。计划见旧仓库 `docs/design/DSH_PROJECT_2026-08-17.md`（本仓库 README 会同步）。
> 前身仓库 `BoenMind`（pi 内核时代）只作参考资产，只读不改造。

## 快速开始

```bash
pnpm install
pnpm dsh web          # 启动 web 版，http://127.0.0.1:3080
```

## 里程碑

- M0 全家桶 bootstrap：官方 dsh-base + dsh-web-app 原样跑通完整会话
- M1 产品外壳：登录/鉴权（dsh-api-gateway）+ 浏览器先行发布
- M2 业务插件面：记忆/皮肤/审计/权限策略
- M3 专家团队 + 管家（team / steward 插件）
- M4 发布：v0.1.0 三平台 + 便携包（Node runtime 内置）

## 目录

```
dsh-home/     DSH_HOME（profiles/ 配置）
plugins/      自研插件（team/steward/skins/memory/audit/browser）
shell/        Tauri 2 桌面壳（后置）
docs/         设计文档
```
