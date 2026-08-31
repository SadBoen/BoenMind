# ADR-0014:W 系列 WebUI 技术路线——assistant-ui 组件库 + OpenAI 兼容插座

- 状态:已裁决(用户 2026-09-01 选型确认;布局蓝本由用户指定)
- 序列:WEBUI 里程碑序列,编号 W1、W2……(M 序列不续用,与 M10 dsh 线
  [ADR-0013 弃用] 划清界限)

## 背景

ADR-0013 弃用 dsh 复刻前端后,新前端选型经三轮收窄:
1. 候选全景(Open WebUI/LobeChat/LibreChat/NextChat/Cherry Studio 等)
2. 用户标准明确为「最适合二次开发」——dsh 教训(改别人的成品=黑盒困境)
   成为否决性判据,排除 Open WebUI(品牌条款+整产品)与 LobeChat(协议
   收紧中+代码量巨大)
3. 决赛 NextChat(88.7k star,MIT,成品)vs assistant-ui(MIT,组件库);
   源码评估:NextChat 为面向个人用户的成品(自带 Node 服务端、41k 行、
   最大单文件 2,171 行,加自家面板=改巨型渲染文件);assistant-ui 为
   开发者组件库(核心 11k 行,ExternalStoreAdapter 标准插口,工具审批
   回调一等公民)
4. 用户指定布局蓝本(三栏开发工具形态:图标栏+会话列表+对话区+工作区
   面板;整页式设置;浅色专业风),并裁决由 AI 出专业设计方案、按后端
   同款严谨度先合约后动工,W 序列独立编号

## 裁决

1. **壳子底座 = assistant-ui**(React/TypeScript 组件库,MIT)。对话流、
   输入框、消息部件用其原语(ThreadPrimitive/ComposerPrimitive/
   MessagePrimitive);接入自研后端用其 ExternalStoreAdapter 标准插口。
2. **对话层插座 = OpenAI 兼容端点** `POST /v1/chat/completions`(SSE 流式
   为主):行业标准接口,任何第三方前端(含 LobeChat 等)都可即插即用,
   壳子可替换零成本。
3. **BoenMind 独家面板**(审批卡片/任务看板/记忆抽屉)在 assistant-ui 中
   以自定义消息部件与旁挂面板实现,为一等公民,不做黑盒 hack。
4. **设计语言**:以用户指定蓝本为准(专业开发工具气质):浅色、细边框、
   小圆角、等宽字体作技术身份特征、蓝色主色/红色危险色、紧凑密度、
   三栏布局+整页式设置。令牌表见 W1 实现规格 §3。
5. 每个组件组入册时必须在 assistant-ui 找到对应原型(找不到的登记为
   「自有组件」并说明),映射表见 W1 实现规格 §5。

## 后果

- 正面:全源码可读可改,无第三方构建产物黑盒;自研 Agent 经标准插座
  对接,壳子可整体替换;审批/任务等独家面有一等公民位置。
- 代价:壳子第一版需自行拼装(侧栏/设置无现成品),首版交付慢于
  fork 成品路线;assistant-ui 迭代快,升级需跟进 breaking change。
- 欠账登记:/v1 端点 W1 免鉴权(与 /api/* 同口径),公网部署前必须补
  Bearer 鉴权(沿 ADR-0009 T-13/T-14)。
