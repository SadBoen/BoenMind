# W2 实现规格:设置中心 + 工作区功能面 + 可拖布局

- 序列:WEBUI W2(前置:W1 对话闭环已通;ADR-0014 技术路线)
- 状态:**已实现并通过浏览器可视化验收(2026-09-01/02,截图
  `milestones/shots-w2/`;验收门 1-5 逐条见 §7)**
- 需求来源:用户逐条口述,本文不改其意图,只做结构化

## 0. 实现裁决登记(2026-09-01 深夜开工时定;裁决问询超时,按推荐项执行)

1. **「插件」对象语义 = 运行时能力提供方**(§1.2 遗留裁决):插件清单 =
   内置能力(编译期注册,系统类,**禁卸载**)+ MCP 服务器组(卸载 =
   移出 mcp.json 配置,重启生效)。MCP 项数据源 = **配置文件全集**
   (loaded/pendingRemoval 标注),与 MCP 管理页同源——否则运行期新增
   条目在插件页不可见、卸载落空(实现期踩实后修正)。PIN = 壳子本地
   偏好(localStorage),PIN 后在设置导航快捷显示,不入后端。
2. **管理面整批暂不入 boenmind-contracts**(§5 表格裁决,推翻原「倾向入」):
   归档线的 config.v0_1 合同是 dsh 协议方法族(ADR-0013 已归档弃用),
   新前端不走该协议;W2 新面 = webapp 壳子私用 REST(`/admin/*`,
   webadmin.rs),以本规格 + `bm-surface-http/tests/webadmin_tests.rs`
   (9 测试)为行为规格。合同只增不破,晚入不亏;W 序列稳定后一次性
   评估入册(登记为入册欠账)。config_store.rs 的 **机制**(model.json
   文件>env、secret 打码、重启生效)按 ADR-0012 原样恢复进主干。
3. **组件实现方式 = 注册表选装 + 少量自有**(沿 §5.1 更新):
   - 选装(shadcn 一键安装/本体搬运):`elements-file-tree`(改造:加
     点击/懒加载交互,去 diff 统计头,行形态保留)、`elements-model-picker`
     (本体搬运,provider 表单模型点选)、surfaces/range 设计语言件、
     shadcn 官方 dialog/button/input/badge/switch/select/label/separator;
   - 注册表 `mcp-config` 原型经查绑定 assistant-ui 浏览器端 MCP 运行时
     (`@assistant-ui/react-mcp`),与 BoenMind 的服务端 mcp.json 管理语义
     不合 → 采用其信息架构(列表+条目+操作对话框),数据面自有;
   - 手写:三栏拖宽分隔条、设置中心整页骨架(左导航+右内容)、
     provider/插件/MCP 业务面、文件预览视图。
4. **tailwind v4 + shadcn 共存策略(§5.1 开工首日定案)**:tailwind v4
   (`@tailwindcss/vite`)+ shadcn new-york + zinc 基色;styles.css 顶部
   `@import "tailwindcss"` + `@theme inline` 桥接;W1 手写令牌原样保留,
   两套变量并存,W3 主题层统一覆盖。

## 0.1 已知边界(验收通过前提下的诚实登记)

- 管理面公开挂载 = W1 同款已登记欠账(单机 localhost 口径,公网前补鉴权)。
- MCP 配置文件中存在无法解析的条目(如 secret 引用缺失)→ 服务器启动
  拒启(M7 既有语义「显式配置硬失败」);界面侧未做启动前预检,登记
  为 W3+ 顺手项(候选:新增条目时校验 secret 引用存在性)。

## 1. 设置中心(齿轮进入,整页式:左侧设置导航 + 右内容区)

### 1.1 模型提供商(增/删/改/查)
- 增:名称 + Base URL + API Key + 模型清单
- 删:移除 provider(密钥一并清除)
- 改:编辑已有 provider 字段
- 查:①连通性测试(对 baseUrl 发真实探针,回显成功/失败)②拉取模型名
  列表(GET /models 真实解析回显)
- 后端依赖:**config API(增删改查+校验)已在 archive/m10-dsh-frontend
  (ADR-0012/config_store.rs),W2 从归档恢复接线**;连通性与模型列表
  端点同批恢复(llm.discoverModels 形态)
- 合同裁决点(实现时定):provider CRUD 接口若冻结字段,入
  boenmind-contracts(surface/webui 或 config schema 扩展);仅壳子内部
  消费则规格约束即可

### 1.2 插件管理(用户创意,结构化转写)
- 顶部:筛选框(按名称过滤)
- 主体:全部插件列表(名称/分类/状态)
- 分类标签(如 系统、内置/编译期插件):**系统与内置类不允许卸载**
- 每项操作:卸载(受分类保护)、设置(进入该项配置)、PIN(PIN 后该项
  在设置页左侧导航列表快捷显示)
- 「插件」的对象语义 BoenMind 侧待裁决:能力提供方(builtin/capability)
  还是界面扩展包——W2 规格定稿时与用户确认一次

### 1.3 MCP 管理
- 结构与插件管理同款:筛选框 + 列表 + 操作
- 后端依赖:MCP 装载/清单(bm-providers mcp.rs 已有装载与校验;管理面
  增删改查接口 W2 新增)
- 合同裁决点:MCP 管理接口同 §1.1

## 2. 工作区功能面(右侧面板升级,或独立工作视图,W2 规格定稿时定布局)

- **目录树管理**:工作区目录的树形浏览(目录展开/进入)
- **文件预览**:目录中单击选中、双击或右键菜单打开 → 预览界面**盖住
  目录树**,预览左上角有**返回图标**回目录树
- 后端依赖:目录列表(host.listDirectory 形态,归档已有)+ 文件读取
  端点(W2 新增;只读,写操作后续)
- 安全:路径穿越防护沿 X-01 先例(lstat 拒链 + realpath 包含校验)

## 3. 聊天界面布局:三栏宽度可拖

- 左(图标栏+会话列表)与中(对话区)、中与右(工作区面板)之间
  各一条可拖分隔条,拖动改列宽(有最小/最大宽度限制)
- 纯前端实现(自有组件),无后端依赖

## 4. 流程纪律:每个里程碑完成后,必须用 ZCode 自带浏览器 MCP 做可视化
验收(截图留档 + 真实操作走查),不能只靠接口测试。(沿前端测试铁律,
已写入验收门;此条为用户明示要求,升格为 W 序列固定流程)

## 5. 合同裁决汇总

| 需求 | 规格 | 入 boenmind-contracts? |
|---|---|---|
| provider CRUD | §1.1 | W2 定稿时裁决(倾向入,沿 ADR-0012 config 先例) |
| 连通性/模型列表 | §1.1 | 同上 |
| 插件管理 | §1.2 | 待「插件」语义裁决后再定 |
| MCP 管理接口 | §1.3 | 倾向入(mcp 目录已有基础) |
| 目录树/文件读取 | §2 | 倾向入(host/browse 形态) |
| 三栏拖宽 | §3 | 纯前端,无合同 |

## 5.1 UI 库存引用(2026-09-01 摸爬)

实现方式更新:组件面从「手写」改为「assistant-ui 注册表选装 + 令牌覆盖」,
盘点与映射见 **milestones/W-ui-inventory.md**(100+ 现成组件,§2 为 W2
直接命中清单:Model Picker/Thread List/Todo list/File tree/Artifact
card/MCP Config Dialog/Settings/Chat panel);手写仅剩三栏拖宽与接线层。
shadcn/tailwind 共存策略 W2 开工首日定案。

## 6. 验收门(全部走浏览器 MCP 可视化验收)

1. 设置:新增 provider → 连通性绿灯 → 拉到模型清单 → 编辑/删除生效
2. 插件/MCP:筛选、PIN 后左侧导航出现、系统类卸载被拒
3. 工作区:目录树浏览 → 打开文件 → 预览盖树 → 返回图标回树
4. 拖动分隔条改列宽,刷新后布局保持(可选,W2 定)
5. 回归:对话闭环(banana 类实测)不退化

## 7. 验收记录(2026-09-01/02 实测,截图 `milestones/shots-w2/`)

| 门 | 结果 | 证据 |
|---|---|---|
| 1 provider | **过** | 真实网关新增 OpenCode Go(33 模型,mimo-v2.5 默认);行内连通测试绿「连通 557ms · 模型 33 个」;编辑改名生效;临时 provider 删除生效(confirm);「设为当前」提示重启生效且 `%APPDATA%\boenmind\config\model.json` 落盘验证(01-provider-added.png) |
| 2 插件/MCP | **过** | 系统内置 7 能力全部真数据,卸载/设置 disabled;筛选 echo→只剩 system.echo;PIN system.danger.purge 后导航出现快捷项(02-plugins-*.png);MCP 类卸载:hook confirm 后 confirm 接受 → `/admin/mcp` servers=[] 落盘验证,「已移出 MCP 配置」提示(07-plugin-mcp-uninstall.png[内容已验,截图通道会话级坏死,IAB 已知怪癖]);新增 wiki 条目落盘+「重启生效」提示(06-mcp-manage.png) |
| 3 工作区 | **过** | 目录树懒展开 docs(nested 文件现形);点 README.md 预览盖树,左上角返回图标回树(03-file-preview-over-tree.png) |
| 4 拖宽 | **过** | 分隔条拖动:会话列 260→340px;刷新后保持 340(localStorage,04-after-reload-layout-persisted.png) |
| 5 对话回归 | **过** | banana 问题真实网关流式回复上屏(「香蕉是一种黄色、弯曲的常见水果。」;05-chat-regression-banana.png);回复后 composer 解锁 |

辅助证据:cargo test --workspace 全绿(含 webadmin 9 测试:CRUD/打码/校验/
探针 stub/MCP 合同校验/路径穿越·绝对路径·符号链接三拒/active 落盘);
clippy 零警告;validate.py 全绿。
