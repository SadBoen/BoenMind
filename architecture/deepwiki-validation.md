# BoenMind 架构基线与三个真实系统的对照验证报告

> 评审对象:《BoenMind 核心架构基线》(`/mnt/dsh_workspace/BoenMind-CORE-ARCHITECTURE.md`,1909 行,下称"基线")。
> 对照系统:Erlang/OTP、Kubernetes、VS Code(证据来自 DeepWiki 索引,查询于本会话内完成,共 14 组 ctx_search)。
> 证据规则:仅引用检索到的原文要点并附 deepwiki URL;检索不到的点如实标注"证据不足",不做推断性补写。

---

## 1. 摘要

- **L0-L5 分层方向被三个系统从不同角度印证**:OTP 用"监督树 + Release Handling"印证了 L0(监督/重启)与"以完整发布物为单位升级";Kubernetes 用"控制面(强类型 API + etcd)/节点面(kubelet 监督循环)"印证了 L1 合同与 L0 监督的分离;VS Code 用"base→platform→editor→workbench 单向依赖 + 多进程模型"印证了"稳定低层、可替换高层"。
- **没有任何一个系统拥有 BoenMind 式的完整 generation 事务状态机**(prepared→migrating→validating→committing→active→draining);最接近的是 OTP relup(步骤化热升级)与 K8s Deployment(新副本逐步替换 + revision 回退),二者粒度和保证都不同。
- **单写者租约是三系统均未显式承诺的约束**:OTP 靠进程内热代码升级天然回避双写;K8s 滚动更新期间新旧副本同时服务;VS Code 扩展近乎无状态。该约束是 BoenMind 的独有加强,而非业界惯例(全局检索未见对应物)。
- **热替换设计(C6/C7)总体成立**:OTP one_for_one 重启、K8s readiness 门控 + kubelet 探针重启、VS Code Extension Host 进程隔离,分别印证了"崩溃检测→摘除→重启→恢复"路径;"校验合同后原子切换 binding + provider.changed 事件"这一精确序列无直接对应物,属 BoenMind 的机制细化。
- **主要风险**:基线的验证期禁止真实外部副作用、8 状态升级事务,比三个被对照系统都严格,实现成本高;建议按第 7 节 S1-S10 吸收三系统的低成本机制(restart 类型、progress deadline、readiness 门控、manifest 前置校验等)。

---

## 2. BoenMind 待验证主张清单

从基线 A(L0-L5 分层)与 B(插件热替换)提炼如下。括号内为基线章节。

| 编号 | 主张 | 基线出处 |
|---|---|---|
| C1 | L0(Bootstrap/Supervisor/Upgrade Manager)是最小且最稳定的控制面,不承载业务逻辑,不依赖待升级的 L2 即可完成代际选择、排空与回滚 | §2.1、§12.1、§13.7 |
| C2 | L1 Kernel Contract(Wire API、事件 Schema、Capability Manifest、Operation 状态机)是稳定根合同,只增不破:Minor 只加向后兼容字段,Major 才允许不兼容并走迁移/兼容桥 | §1.1、§2.3、§5.2、§13.5 |
| C3 | L2 Runtime Core 以 generation(而非单个插件)为升级单位,升级事务经历 prepared→migrating→validating→committing→active→draining(另有 rolled_back/failed)状态机 | §2.2、§13.4 |
| C4 | 升级验证期隔离:新代际在隔离状态副本中只读恢复,禁止真实外部副作用,验证通过后才原子切换 active 指针;失败则保持旧代际服务 | §13.4 |
| C5 | 单写者租约/写入栅栏:同一时刻只有一个 generation 对同一份可写业务状态拥有写入权;旧代际排空期间默认只读 | §2.2、§13.4 |
| C6 | Provider 热替换序列:draining(拒新请求、等待/取消短请求)→ 新 Provider handshake → 校验能力合同与健康 → Registry 原子切换 binding → 发布 provider.changed → 停旧;调用方只依赖稳定 Capability 名称 | §6.4、§13.1、§13.6 |
| C7 | Provider 崩溃恢复:Supervisor 检测 → Registry 标记 unavailable → 发布 provider.crashed → 新请求得到明确 unavailable → Supervisor 重启 → 重新 handshake/register → 恢复 binding;调用方不无限等待 | §13.2、§12.3(quarantined) |
| C8 | 崩溃隔离与多语言支持以独立进程为主要手段,进程由 L0 Supervisor 树管理;可信高频组件允许进程内实现但失去崩溃边界 | §2.1、§4.3、§12.1、§12.2 |

---

## 3. Erlang/OTP 对照

证据源:https://deepwiki.com/erlang/otp/1.2-otp-design-principles 、https://deepwiki.com/erlang/otp/2.1-process-management 、https://deepwiki.com/erlang/otp/6.2-application-version-management

### 3.1 逐层映射表

| BoenMind 层 | OTP 对应物 | 证据要点 |
|---|---|---|
| L0 Supervisor/Upgrade Manager | OTP supervisor(监督树)+ SASL release_handler | supervisor 行为由 `child_spec()` 定义,含启动函数(MFA)、restart type(`permanent/transient/temporary`)与 shutdown 超时(supervisor.erl#L51-L55);release_handler 负责解包与安装 release(kernel.app.src#L172-L173) |
| L1 Kernel Contract | behaviours(gen_server/gen_statem 等回调合同)+ appup/relup 版本迁移合同 | behaviours 是"通用部分 + 开发者回调部分"的模式形式化;gen_server 封装状态并提供同步 call/异步 cast(gen_server.erl#L26-L32);Appup 定义应用在版本间如何升级/降级,Relup 定义不停机升级整节点的步骤(Hot Code Swap) |
| L2 Runtime Core(generation) | Release(应用集 + ERTS + 配置)整体升级 | "Releases are functional Erlang systems containing a set of applications, the ERTS, and configuration"(stdlib.app.src#L23-L130)——升级单位与 BoenMind 的 generation 同构:都是"完整运行时代际",不是单个插件 |
| L3 Providers/服务 | OTP application/监督树下的 child 进程 | 子进程按 child_spec 由监督者启动、重启,shutdown 超时控制退出宽限期 |
| L4 User Apps | OTP application(业务应用) | 应用是 OTP 的部署与生命周期边界,与 BoenMind 的 App 域同位 |
| L5 Surfaces | (无直接对应) | OTP 无交互适配层概念,证据不足,不做映射 |

### 3.2 内核/合同/扩展点划分对照

- OTP 的"内核合同"是 behaviours(回调协议)与 OTP 应用结构;"扩展点"是符合 behaviour 的回调模块与 application。这与基线 §2.3"内核只由合同与最小机制组成"的判定同构:接入方式 = 实现既有合同,而不是修改内核。
- appup/relup 把"版本迁移方式"也做成声明式合同(输入版本→输出版本的步骤),与基线 §13.4"迁移脚本必须声明输入版本、输出版本、失败清理方式"的要求相互印证。
- 差异:OTP 的合同稳定性靠社区纪律与兼容文档维护,基线的"只增不破 + Minor/Major 分级"是更显式的合同治理。

### 3.3 热替换/升级/回滚机制对照(机制级)

- **升级状态机**:OTP relup 是有序指令脚本(逐步 suspend→load_code→code_change→resume 的组合,由 appup 描述),但没有 BoenMind 式的 prepared→validating→committing→draining 显式事务状态,也没有"验证期只读副本"概念。→ 部分印证 C3:升级单位同构,状态机粒度不同。
- **验证**:OTP 侧的证据是升级被严格测试——release_handler_SUITE 覆盖"升级监督者及其子进程""校验 appup/relup 语法与一致性"(release_handler_SUITE.erl#L104-L105、#L68);ssl 应用因内部状态变化"major version jump 常需要重启"(ssl.appup.src#L9-L20)。这印证了 C4 的前半(迁移路径先验证、不可逆变更需重启窗口),但没有"验证期禁止真实外部副作用"的运行时强制。→ BoenMind 更严格。
- **排空(draining)**:child_spec 的 shutdown 超时是每个子进程的优雅退出宽限期;未检索到"旧代际拒收新请求 + 等待短请求完成"的节点级排空协议。→ C5 的"排空默认只读"在 OTP 无对应物;不过 OTP 通过**进程内**热代码升级(同一进程换新代码、状态经 code_change 迁移;code_change 细节本次证据不足)天然避免了"两个写入者"问题——它的单写者是"同一个进程",而 BoenMind 选择"两个进程、一个租约",因此必须显式引入租约。→ C5 裁决见第 6 节:OTP 以另一机制达成了同一目标。
- **回滚**:appup 同时定义 upgrade 与 downgrade 路径,release_handler 支持安装 releases;未检索到"切换后回退 active 指针 + 快照恢复"的细节(证据不足)。→ C1 的"不依赖 L2 即可回滚":OTP 的 release_handler 运行在节点内(它属于基础 release 而非业务应用),监督树崩溃时重启靠节点自身,不是 BoenMind 设想的"节点外最小控制面"。方向印证,隔离强度上 BoenMind 更激进。
- **崩溃恢复(C7)**:链接是双向连接,监视器是单向并在被监视进程终止时发送 `'DOWN'` 消息(bif.c#L134-L257、erl_bif_info.c#L233-L243);进程终止时 `erts_do_exit` 处理清理、向链接传播信号、通知监视器(erl_process.c#L3500-L3700);信号专用队列保证进程繁忙时退出/链接/监视信号仍被处理(ErtsProcSigQueue)。监督策略谱系:`one_for_one`(只重启崩溃子进程)、`one_for_all`、`rest_for_one`(重启失败子进程及其后启动的全部子进程)、`simple_one_for_one`(动态版 one_for_one)(supervisor.erl#L74-L89)。→ 这直接印证 C7 的"检测→重启→恢复",且 one_for_one 语义与基线"单个 Provider 崩溃不拖垮其他 App"一致。restart intensity(max_restarts/强度窗口)本次未检索到细节,证据不足;基线 §12.3 的 quarantined(崩溃过多自动隔离)与 OTP 的"重启强度超限则监督者自杀"意图相近但参数未证实。

### 3.4 更严格/更宽松/风险与启示

- **BoenMind 更严格处**:验证期禁真实副作用、显式 generation 事务状态、跨代单写者租约、统一 unavailable 错误信封(OTP 调用方看到的是进程不存在/exit 原因,没有合同级错误码)。
- **OTP 更强处**:热代码升级粒度可到单个模块;50 年验证的监督策略谱系(尤其 rest_for_one 的依赖顺序重启)。
- **风险提示**:BoenMind 的两进程 generation 切换比 OTP 的进程内升级多出"双代际共存窗口",若租约实现有缺陷,会出现 OTP 中不存在的双写故障类别。
- **启示**:child_spec 的 restart type + shutdown 超时是低成本高价值的 manifest 字段(见 S1、S7)。

---

## 4. Kubernetes 对照

证据源:https://deepwiki.com/kubernetes/kubernetes/1-overview 、https://deepwiki.com/kubernetes/kubernetes/4.1-kubelet-architecture 、https://deepwiki.com/kubernetes/kubernetes/3.3-controller-manager

### 4.1 逐层映射表

| BoenMind 层 | Kubernetes 对应物 | 证据要点 |
|---|---|---|
| L0(系统级控制面) | 双重结构:控制面(API server/scheduler/controller-manager)+ 节点面 kubelet | 控制面组件经 API server 协调并把状态持久化到 etcd;kubelet/kube-proxy watch API server 并 reconcile 本地状态(k8s Overview#Component Interaction Diagram) |
| L0 Supervisor(就近监督) | kubelet | kubelet 是节点上的容器监督者:pod worker 子系统为每个 pod 维护专用 goroutine、顺序处理生命周期更新(4.1-kubelet-architecture#Pod Workers and Sync Loop) |
| L1 Kernel Contract | kube-apiserver 的强类型 API 对象 + 校验 | "kube-apiserver 是中心枢纽:暴露 API、校验并处理请求、把状态持久化到 etcd";"强类型 API 对象 + validation"(Overview#kube-apiserver、#Summary) |
| L2 Runtime Core | (无单一对应;etcd + 控制器组合承担规范状态) | 状态不入进程内存而入 etcd,控制器从声明状态推导行为——与基线 §2.2"状态不能只存在进程内存里"同向 |
| L3 Providers | 容器运行时/CNI 等节点扩展(经 kubelet 驱动) | kubelet 按容器规格驱动探针与状态更新(prober worker→statusManager) |
| L4 User Apps | Pod/容器内应用 | Deployment/ReplicaSet 管理期望副本(3.3-controller-manager) |
| L5 Surfaces | kubectl 等 API 消费者(经同一 API) | 未经本次检索细节证实,只作方向性映射 |

### 4.2 内核/合同/扩展点划分对照

- K8s 的"L1 合同"是 API 对象 Schema + 校验 + etcd 持久化;一切组件(含 kubelet)通过 watch/声明式调和协作,没有组件直接改写他人状态。这与基线"调用方只依赖稳定 Capability 名称、状态归属清晰"的主张同构,且把合同做成了机器校验的对象(与基线 M0"合同可机器校验"一致)。
- 扩展点:Deployment→ReplicaSet→Pod 的分层期望模型里,上层只声明期望,下层控制器负责兑现——对应基线 §13.6"局部升级不动整体"的思想。

### 4.3 热替换/升级/回滚机制对照(机制级)

- **kubelet 监督循环 vs L0(重点)**:pod worker 每 pod 一个 goroutine、顺序处理更新;probeManager 为 Liveness/Readiness/Startup 每个探针建专用 worker(周期/超时),经 Exec/HTTP/TCP/gRPC 执行,结果写入 statusManager(SetContainerReadiness/SetContainerStartup)。这印证 C1 的"监督者负责启动/监控/重启"与 C7 的"检测失败→处置":探针失败 ⇒ 就绪性摘除或容器重启,是"崩溃→标记→重启→恢复"的声明式版本。与 BoenMind 差异:K8s 的监督是**控制循环收敛**(期望 vs 实际),L0 是**事件驱动监督**(崩溃即动作);kubelet 自身不被 pod 内机制监督,其可用性由控制面/静态 pod 保障——细节本次证据不足。
- **Deployment 滚动更新/回滚 vs generation 事务(重点)**:deployment 控制器内实现了 rolling.go、recreate.go、rollback.go、progress.go、sync.go 及对应测试(3.3-controller-manager 文件列表),证明"新版本逐步替换旧副本、可回退、可探测 rollout 停滞"是真实的机制组合。与 C3/C4/C5 对照:
  - 滚动更新 ≈ BoenMind 的"新 generation validating→active、旧 generation draining"的**流量级**版本;但 K8s 新旧 pod **同时服务**,没有跨版本单写者约束 → C5 在 K8s 无对应,属 BoenMind 加强(其代价是 K8s 假设副本无状态)。
  - rollback.go 的 revision 回退 ≈ 基线 §13.4 的"回退 active 指针";但 K8s 回滚是"把期望改回旧 revision",没有"迁移可逆则反向迁移、不可逆则快照恢复"的状态迁移语义 → C4/C3 的状态迁移部分 BoenMind 更严格。
  - progress.go 表明存在 rollout 卡死检测;max surge/max unavailable 具体参数语义本次未检索到(证据不足)。
  - 探针就绪性摘除(Probing→statusManager)≈ C6 的 draining"拒新请求":先把实例从就绪集合摘除再终止,是"原子切换 binding"的分布式对应物;provider.changed 事件 ≈ endpoint/controller 的事件传播,细节证据不足。
- **API server 声明式合同 vs L1(重点)**:API server"校验请求并持久化到 etcd"、组件 watch 后本地 reconcile——说明"稳定合同 + 声明状态 + 就地恢复"是可运维的架构。基线 L1 把 Wire API/事件 Schema/Operation 状态机列为只增不破合同,方向被印证;但 K8s 的合同兼容性治理(API 约定/弃用策略)本次未检索到原文(证据不足),不能直接佐证"只增不破"的版本化条款。

### 4.4 更严格/更宽松/风险与启示

- **BoenMind 更严格处**:验证期禁真实副作用(K8s 滚动更新中新副本直接接真实流量,靠 readiness 探针兜底);跨代单写者租约(K8s 明确允许多版本并存服务);升级单位是整个 L2(K8s 可按 Deployment 细粒度)。
- **K8s 更强处**:声明式调和让"崩溃恢复"不需要专门升级窗口;探针体系(Liveness/Readiness/Startup 三分)比 BoenMind 的 verification 钩子更成体系。
- **风险提示**:BoenMind 单写者租约若与"验证期只读副本"叠加,验证覆盖面受限于只读探针,可能漏掉只在真实写负载下暴露的迁移缺陷——K8s 用真实流量金丝雀换取覆盖面,二者是不同取舍。
- **启示**:progress deadline 式停滞检测、readiness 先摘流后停进程、探针三分法(对应 verification/liveness 分层)都值得吸收(见 S3、S4、S9)。

---

## 5. VS Code 对照

证据源:https://deepwiki.com/microsoft/vscode/1.2-core-architectural-layers 、https://deepwiki.com/microsoft/vscode/1-vs-code-architecture-overview 、https://deepwiki.com/microsoft/vscode/5-extension-system 、https://deepwiki.com/microsoft/vscode/5.1-extension-host-architecture

### 5.1 逐层映射表

| BoenMind 层 | VS Code 对应物 | 证据要点 |
|---|---|---|
| (对照物:代码分层) | base→platform→editor→workbench→sessions 单向依赖 | "架构遵循单向依赖流";base 为通用工具、不依赖其他层;platform 为共享服务;editor(Monaco)可脱离 workbench 独立使用(1.2-core-architectural-layers#Layering Model),且有 Layering Rules and Enforcement |
| L0(应用生命周期) | Electron Main Process | 管理应用生命周期、窗口创建、原生 OS 集成(CodeApplication/WindowsMainService)(1-vs-code-architecture-overview#Multi-Process Model) |
| L1(合同) | extHost.protocol 双向 RPC 契约 + 扩展 manifest | `IMainContext`(Ext Host→Main 的 RPC 形状)、`IExtHostContext`(Main→Ext Host 的 RPC 形状)、`IExtensionDescription`(package.json manifest 数据)(5-extension-system#Important Interfaces) |
| L2(核心服务) | workbench 核心服务 + ExtensionDescriptionRegistry | AbstractExtensionService 读 manifest 并填充 ExtensionDescriptionRegistry(5-extension-system#Extension Lifecycle) |
| L3/L4(扩展承载) | Extension Host / Utility Processes | 扩展运行在专用进程以防阻塞 UI;Utility Processes 承载 PTY host、文件监视、搜索等(1-vs-code-architecture-overview#Multi-Process Model) |
| L5 Surfaces | 每窗口一个 Renderer(Workbench UI + Monaco) | Renderer 承载 UI;扩展的重计算不进 UI 线程(同上) |

注意:VS Code 的"层"是**代码依赖分层**(静态、同进程内纪律),BoenMind 的 L0-L5 是**运行时进程/合同分层**(动态、跨进程);二者同构点是"低层稳定、高层可替换",但不能画等号。

### 5.2 内核/合同/扩展点划分对照

- 扩展生命周期五阶段:Discovery(磁盘/marketplace 扫描)→ Registration(读 manifest 填充 Registry)→ Activation(按 activationEvents 懒激活,由 ExtHostExtensionService 管理)→ Execution(经 RPCProtocol 代理调用 workbench)→ Termination(workbench 关闭时向扩展宿主进程发信号清理)(5-extension-system#Extension Lifecycle)。这与基线 §12.3 的 registered→installed→enabled⇄disabled→uninstalled 生命周期同构,且"懒激活"是基线没有的启动策略。
- manifest(IExtensionDescription,package.json)声明的是**贡献点**(contributes)与引擎兼容性,不是权限声明;扩展默认获得宽泛宿主 API。基线的 Plugin manifest 要求声明 capabilities/data_domains/secret 引用并经安装审批——**比 VS Code 更严格**。VS Code 侧权限/签名治理细节本次证据不足。
- 安装/市场:IExtensionGalleryService 查询与下载 VSIX,IExtensionManagementService 负责安装/卸载/磁盘扫描(5-extension-system#Extension Marketplace and Management)——对应基线"插件市场是非目标、仅本地安装"的差异点,机制可复用于本地安装器。

### 5.3 热替换/崩溃隔离机制对照(机制级)

- **多进程模型 vs L0-L5(重点)**:Main(生命周期)、Renderer(每窗口)、Extension Host(扩展专用进程)、Utility Process(PTY/文件监视/搜索)、REH(远程扩展宿主)——"确保扩展或语言服务做重计算时 UI 仍响应"是全部进程拆分的动机。这印证 C8:独立进程是崩溃隔离与语言异构的标准手段;且 VS Code 也保留"进程内实现"选项(同进程组件),与基线 §12.1/§12.2 的双轨一致。
- **Extension Host 崩溃隔离 vs 插件进程(重点)**:Extension Host 是"与主 workbench UI 隔离的专用执行环境,保证昂贵的扩展操作不阻塞 UI 线程"(5.1-extension-host-architecture);ExtensionHostManager 包装 IExtensionHost,start() 返回 IMessagePassingProtocol(5.1#Process Management)。粒度差异:VS Code 隔离单位是**整个扩展宿主进程**(所有扩展共享一个宿主),BoenMind 的 Provider 可以**逐个进程**;崩溃时 VS Code 的受影响面是全部已激活扩展,BoenMind 单 Provider 崩溃影响面更小。扩展宿主崩溃后的自动重启/降级策略细节本次未检索到(证据不足),不能对比 C7 的完整序列。
- **扩展 manifest/生命周期 vs Plugin manifest(重点)**:见 5.2。热替换方面,证据显示扩展变更通常经宿主进程生命周期处理(Termination 发信号清理),未检索到"逐扩展 draining→原子切换 binding"的机制(证据不足)→ C6 在 VS Code 无对应物,BoenMind 的 Registry 原子切换 + provider.changed 是超出 VS Code 的设计。
- 单写者/升级:扩展近乎无状态(状态在 workbench/存储服务),不存在跨代双写问题 → 与 C5 的对比同 K8s:无对应约束,BoenMind 面向的是有状态 Provider 场景。

### 5.4 更严格/更宽松/风险与启示

- **BoenMind 更严格处**:manifest 权限声明 + 安装审批 + 崩溃隔离粒度(逐 Provider 进程)+ quarantine;VS Code 扩展拿到的是宽泛 API,信任模型更宽松。
- **VS Code 更强处**:懒激活(activationEvents)降低常驻成本;manifest 在 Registration 阶段就被 Registry 消化(坏 manifest 在注册期暴露);双向 RPC 契约(IMainContext/IExtHostContext)把接口按方向分表,防止单接口膨胀。
- **风险提示**:BoenMind 若把所有 Provider 塞进少数共享宿主进程(阶段一倾向),会退化回 VS Code 的粒度——崩溃隔离主张(C8)只在逐进程拆分兑现。
- **启示**:manifest 前置校验(注册期拒绝)、懒启动扩展点、RPC 按方向分合同(见 S5、S6、S8)。

---

## 6. 逐条主张裁决表

| 编号 | 主张(简) | 裁决 | 一句话理由 |
|---|---|---|---|
| C1 | L0 最小控制面,不依赖 L2 即可排空/回滚 | 部分确认 | OTP release_handler/监督树与 kubelet 监督循环印证"独立监督+发布安装"职责,但两者监督机制都内嵌于被升级系统(同节点/同二进制体系),BoenMind 的"控制面独立于待升级 L2"是超出三系统的隔离要求,无反例也无同强度先例 |
| C2 | L1 合同只增不破(Minor 加字段/Major 迁移) | 部分确认 | VS Code 单向分层与 K8s 强类型 API+校验印证"稳定根合同是系统底座";但三系统合同兼容性治理的具体条款(API 弃用策略等)本次证据不足,"只增不破"的版本化承诺未获直接原文支撑 |
| C3 | generation 升级事务状态机(6+2 状态) | 部分确认 | OTP release(应用集+ERTS 整体升级、appup/relup 步骤化)与 K8s Deployment(新副本替换/回退/停滞检测)印证"升级单位+回退+探测"三要素,但无任何系统拥有 BoenMind 式显式 8 状态事务 |
| C4 | 验证期隔离:只读副本恢复、禁真实副作用、通过后原子切换 | 部分确认 | OTP 升级测试(release_handler_SUITE 验证 appup/relup 一致性、ssl 大版本需重启)印证"迁移路径先验证";但"验证期禁止真实外部副作用"在三系统均无对应(K8s 新副本直接接真实流量),属 BoenMind 加强 |
| C5 | 单写者租约/写入栅栏 | 部分确认 | OTP 以进程内热升级天然达成单写者(同进程换代码),K8s/VS Code 无此约束(新旧副本并存/扩展无状态);三系统无一要求 BoenMind 式跨代租约——它是对"两进程 generation"方案的必要配套,而非业界惯例 |
| C6 | Provider 热替换:draining→handshake→校验→原子切换 binding→provider.changed→停旧 | 部分确认 | K8s readiness 摘流后再终止 ≈ draining+binding 切换的分布式版本;但"校验能力合同后原子切换 + 显式变更事件"的完整序列无系统对应,VS Code 逐扩展热替换证据不足 |
| C7 | 崩溃:检测→unavailable→provider.crashed→明确错误→重启→重握手→恢复 | 确认 | OTP one_for_one 监督重启 + link/monitor('DOWN' 信号)与 K8s kubelet 探针+restart 的"检测→摘除→重启→恢复"路径完整印证;"明确 unavailable 错误信封"为 BoenMind 增强,quarantine 对应的 OTP 重启强度参数证据不足但不影响主路径裁决 |
| C8 | 独立进程是崩溃隔离与多语言边界,由 Supervisor 树管理 | 确认 | VS Code 多进程模型(ExtHost/Utility Process 专为隔离扩展与 UI)与 K8s pod 边界、OTP 进程隔离一致印证;"允许进程内实现但失去崩溃边界"的宽松条款与 VS Code 单宿主共享进程的取舍相同 |

---

## 7. 对基线的修订建议

以下均为建议,不构成结论;每条注明来源系统与证据。

- **S1**:在 Plugin/Provider manifest 中引入显式 restart 类型与退出宽限期字段(对标 OTP child_spec 的 `permanent/transient/temporary` + shutdown timeout),替代单一"quarantined(崩溃过多)"模糊语义。(来源:dw-otp-design,supervisor.erl#L51-L55)
- **S2**:把"升级迁移的回放测试"写进 M2/M8 验收:对标 release_handler_SUITE 对 appup/relup 语法与一致性的校验,为 generation 迁移脚本建立"输入版本→输出版本→失败清理"的机器校验用例。(来源:dw-otp-versions,release_handler_SUITE.erl#L68、#L104-L105)
- **S3**:为 generation 升级事务增加 progress deadline 式停滞检测:validating/migrating 超时无进展即自动 abort 并保持旧代际(基线 §13.4 目前只有 failed 状态,无停滞判定)。(来源:dw-k8s-controller,deployment progress.go 机制存在;K8s 具体参数语义证据不足,建议只吸收"停滞检测"思想)
- **S4**:把 C6 的 draining 细化为两步:先从 Registry 发现结果摘除旧 binding(调用方不再被路由),再等待排空并停进程;并对不可排空的长请求定义 deadline 取消(基线 §13.1 已有雏形,建议明确"摘除"与"终止"是两个动作)。(来源:dw-k8s-kubelet,prober→statusManager 就绪性摘除)
- **S5**:Registry 增加 manifest 前置校验阶段:manifest 解析失败、版本/引擎不兼容在**注册期**即拒绝并进 quarantined 分表,而不是推迟到 handshake 失败。(来源:dw-vscode-ext-system,AbstractExtensionService 在 Registration 阶段填充 ExtensionDescriptionRegistry)
- **S6**:为 Provider/App 增加懒启动扩展点(对标 activationEvents):低频 Provider 可声明由事件/首次调用触发启动,降低常驻进程数量;与 C8 的独立进程主张不冲突。(来源:dw-vscode-ext-system,Extension Lifecycle#Activation)
- **S7**:L0 Supervisor 规则中允许按依赖顺序的级联重启策略(对标 OTP `rest_for_one`:失败子进程及其后启动的依赖一并重启),用于 App 内多 Provider 有启动依赖的场景;默认仍为 one_for_one。(来源:dw-otp-design,supervisor.erl#L74-L89)
- **S8**:L1 Wire API 按"调用方向"拆分合同分表(对标 IMainContext/IExtHostContext 双向 RPC 契约),避免单一接口文件同时承载 Surface→Runtime 与 Runtime→Provider 两个方向导致膨胀。(来源:dw-vscode-ext-system,extHost.protocol.ts)
- **S9**:把基线 §5.2 的 verification 钩子分层对标 K8s 探针三分法:Liveness(该不该重启)/Readiness(该不该接流)/Startup(何时开始探测),分别映射到"崩溃处置、binding 摘除、启动宽限"三个不同动作,避免单一健康检查语义过载。(来源:dw-k8s-kubelet,probeManager 的 Liveness/Readiness/Startup 分离)
- **S10**:在 §13.5 Patch 级定义中显式承认"维护窗口重启切换"是合法机制(对标 ssl 应用大版本升级选择重启而非热迁移),避免把热升级当成所有 Patch 的默认要求。(来源:dw-otp-versions,ssl.appup.src#L9-L20)

**证据不足清单**(未纳入上述建议依据):K8s max surge/max unavailable 参数语义、K8s API 合同弃用/兼容策略原文、OTP gen_server code_change 回调细节与 restart intensity 参数、VS Code 扩展宿主崩溃后自动重启策略、VS Code 逐扩展热替换机制、"单写者租约/generation lease"在三系统中的对应物(全局检索无结果)。

---

## 8. 参考链接

- Erlang/OTP:
  - https://deepwiki.com/erlang/otp/1.2-otp-design-principles (监督树、监督策略、child_spec、Release Handling、behaviours、gen_server)
  - https://deepwiki.com/erlang/otp/2.1-process-management (link/monitor、'DOWN' 信号、erts_do_exit、信号队列)
  - https://deepwiki.com/erlang/otp/6.2-application-version-management (release_handler_SUITE、appup/relup 一致性测试、ssl 大版本重启)
- Kubernetes:
  - https://deepwiki.com/kubernetes/kubernetes/1-overview (控制面/节点面、kube-apiserver、etcd、声明式调和)
  - https://deepwiki.com/kubernetes/kubernetes/4.1-kubelet-architecture (pod workers/sync loop、探针子系统、statusManager)
  - https://deepwiki.com/kubernetes/kubernetes/3.3-controller-manager (deployment rolling/recreate/rollback/progress/sync、HPA)
- VS Code:
  - https://deepwiki.com/microsoft/vscode/1.2-core-architectural-layers (base→platform→editor→workbench→sessions、分层规则、DI、Registry 模式)
  - https://deepwiki.com/microsoft/vscode/1-vs-code-architecture-overview (Main/Renderer/Extension Host/Utility Process 多进程模型)
  - https://deepwiki.com/microsoft/vscode/5-extension-system (IExtensionDescription、生命周期五阶段、ExtensionDescriptionRegistry、市场服务)
  - https://deepwiki.com/microsoft/vscode/5.1-extension-host-architecture (ExtHost 隔离、ExtensionHostManager、IMessagePassingProtocol)

---

*本报告仅依据上述 DeepWiki 索引证据与基线原文撰写;未检索到的机制一律标注"证据不足",未做外推。生成于 2026-08-28 架构评审任务。*
