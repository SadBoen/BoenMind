# 新加坡 MPA（mpa.gov.sg）— curl 直抓即可

**难度 1/5。完全服务端渲染，无需任何浏览器。**

## 核心事实

- curl + 完整 UA 直接 200，421KB 全量 HTML，首页含 Circulars/Events/What's New/newsroom 全部链接
- 这类政府机构官网**先试 curl / web_fetch**，通常比预期简单

## 关键数据源

### RSS 聚合源（通告类数据首选）

```
https://www.mpa.gov.sg/feeds/media-releases
```

- RSS 2.0，~50 条，含 **Port Marine Notices / Notices to Mariners / media releases / speeches**
- 每条含标题+日期+链接，近实时更新（2026-08-09 实测 lastBuildDate 当日）
- 通告/公告类数据一条 RSS 全量拿，比爬列表页省事，可配 cron 增量监控
- 抓取：`curl -s "https://www.mpa.gov.sg/feeds/media-releases"`（XML）

### 通告详情页

```
/media-centre/details/<slug>
```

### 船舶注册（SRS）信息区

```
/singapore-registry-of-ships/register-with-srs/
   ├── pre-registration-procedure   注册前流程（船名预留→官方编号+呼号）
   ├── type-of-registrations        注册类型（临时/永久/光船出租等）
   ├── statutory-certificates       MPA 签发法定证书（CSR 等）
   ├── fee                          费用（初始 S$2.50/NT，min S$1250；年税 S$0.20/NT）
   ├── application-forms            申请表 PDF
   └── faq
```

## 用户业务背景（2026-08-09 提及）

用户有船要申请证书——MPA 相关可能是船舶注册（SRS）/船员证书（CoC）。
**但注意**：用户业务背景只是解释"为什么需要数据"，不等于授权直接做领域调研，动手前先确认要哪个方向（铁律 4）。
