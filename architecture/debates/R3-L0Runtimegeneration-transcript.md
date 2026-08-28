# Zen Consensus 辩论记录:R3 L0 独立控制面 + Runtime generation 升级回滚

辩论对象:§17 裁决 R3「L0 独立控制面 + Runtime generation 升级回滚」

> 协议:Zen consensus——三个不同模型家族担任三方(FOR=架构师钢人论证,AGAINST=安全可靠性挑战,EMPIRICAL=真实系统实证裁决),两轮(独立立场→交叉质证),缺席与不可裁决的分歧如实记录。

角色分配:EMPIRICAL=gemini-3.7-flash, FOR=glm-5-turbo, AGAINST=gpt-5.6-luna

## 裁决原文

裁决:L0 Supervisor/Upgrade Manager 是最小且最稳定的独立控制面,不承载业务逻辑,不依赖待升级的 L2 即可完成回滚;Runtime Core 以完整代际(generation)为升级单位:安装到独立版本目录→校验→快照/检查点→新代际 validating(隔离状态副本只读恢复,禁止真实外部副作用)→取得单写者租约→原子切换 active→旧代际 draining→停止并保留回滚材料;升级失败保持 v1 服务;切换后异常可回退 active 指针;升级目标是语义连续而非内存连续,无法判断的外部副作用进入 outcome_unknown。

## 第一轮:独立立场

### 【EMPIRICAL】

```json
{
  "conceded_risks": [
    "隔离验证存在覆盖盲区:禁止真实外部副作用(§13.4 L1233)意味着写路径与真实模型调用在切换前零覆盖,且单机无 canary 部分放量,v2 的第一次真实写入发生在从未演练的路径上——这是 R3 比主流渐进暴露模式更弱的一点,也是对'验证通过才切换'承诺的最大反驳。",
    "'切换后异常可回退'缺少异常判定标准与自动回滚机制:Android 以 boot_success 超时触发自动回滚,R3 未定义等价的 health 判据与自动/手动边界(基线通篇未给出),回退实际可能依赖用户感知故障后手动执行 boenmind runtime rollback,把发现故障的延迟转嫁给单用户。",
    "语义连续的产品级代价:每次升级把 in-flight 回合标记 interrupted,对声称'长期运行的个人 AI Runtime'而言,升级中断若与升级频率相乘会成为常态体验;而 draining 只保护'不可重复的收尾事务'(§2.2 L157),模型生成流直接标记 interrupted(§13.4 L1261),宽松度可能不足以支撑其连续性承诺。"
  ],
  "evidence": [
    "Android A/B(seamless)系统更新:运行系统写 inactive slot→boot_verifier 做 dm-verity 校验→markBootSuccessful() 前失败自动回退旧 slot,文档明言 ChromeOS 亦成功采用同模式(https://source.android.com/docs/core/ota/ab)——对应独立版本目录→校验→原子切换→保留回滚材料全链。",
    "NixOS:nixos-rebuild switch 原子切换 /nix/var/nix/profiles/system 符号链接,可 rollback 到任意 generation(https://nixos.wiki/wiki/Overview_of_the_NixOS_linux_distribution;https://github.com/NixOS/nixpkgs/issues/24374)——'active 指针原子切换'的直接先例。",
    "Firefox 更新 staging:运行中把安装目录复制为副本打补丁,重启时以 Replace Request 原子换入(https://firefox-source-docs.mozilla.org/toolkit/mozapps/update/docs/Concepts.html)——但其验证仅为文件级,不及 BoenMind 的运行时隔离验证,证明 R3 该环节超出桌面软件先例。",
    "Erlang/OTP release_handler 与 appup 支持 in-place 升级/降级并以 code_change 保留进程状态(https://www.erlang.org/doc/system/release_handling.html);社区实践公认 relup 脆弱、普遍转向外置状态+重启(https://learnyousomeerlang.com/relups)——R3 放弃内存连续正与该行业收敛一致。",
    "Kubernetes 滚动更新:readiness 通过即接真实流量,revisionHistoryLimit+kubectl rollout undo 回滚;systemd socket activation 以文件描述符交接实现近零停机换手——分别对应'新代际接真实流量'与'单机换手'的服务端先例(https://oneuptime.com/blog/post/2026-01-30-deployment-rolling-configuration/view)。",
    "Kafka controller epoch fencing 与 KRaft 单 active 控制器防 split-brain(https://developers.redhat.com/articles/2025/09/17/deep-dive-apache-kafkas-kraft-protocol)——generation lease/单写者租约(§13.4 L1260)的分布式先例。",
    "基线:§2.1 L119-123(L0 最小控制面不承载业务逻辑);§2.2 L144(重启/换代必须从持久状态恢复)、L157(旧代际排空期默认只读);§13.4 L1231-1237(validating 只读恢复/禁止真实副作用/保留回滚材料)、L1261-1268(生成流标记 interrupted、outcome_unknown);§13.7 L1323(回滚不依赖 L2);§14.1 L1398(L2 不可用时 CLI 仍可执行排空/回滚)。"
  ],
  "key_arguments": [
    "R3 的主干流程(独立版本目录安装→校验→原子切换 active 指针→旧代际保留作回滚材料)与 Android/ChromeOS A/B slot、NixOS generation 符号链接切换、Firefox staging 目录换入逐一同构,是数十亿设备验证过的模式,不构成激进设计。",
    "'L0 不依赖待升级 L2 完成回滚'(§13.7 L1323)对应 Android update_engine 不随 OTA 更新自身与 Windows servicing stack 先行的既有约束,且有 apt/包管理器自升级失控的反例佐证其必要性。",
    "BoenMind 比 K8s 与 Firefox 更严格:K8s 新 Pod readiness 通过即接真实流量、Firefox 只做文件级 staging,而 R3 要求 validating 期在隔离只读状态副本上恢复并禁止真实外部副作用(§13.4 L1231-1233);直接后果是写路径在切换前从未被真实执行验证,而单机没有 canary 梯度可弥补。",
    "BoenMind 比 Erlang/OTP 更宽松:OTP relup 用 code_change 保持进程内存状态,R3 明确只要语义连续、放弃内存连续(§13.4 L1263-1266);这与行业从 relup 全面转向'外置状态+重启换新'的收敛一致(§2.2 L144 本就要求从持久状态恢复),但意味着每次升级 in-flight 回合必然标记 interrupted,连续性承诺完全押在 generation.changed/agent.interrupted 恢复事件上。",
    "单写者租约作为'切换完成'判据(§13.4 L1260)是 Kafka controller epoch fencing 的单机同构物,有坚实先例;但在单机上它引入了主流桌面先例(systemd/VS Code 的'重启即换')刻意规避的新失败模式——租约异常即自禁写,可用性换一致性是明确的严格化代价。",
    "实证上最薄弱处:OTP relup 即使有严格框架仍以状态形状迁移 bug 闻名失败,R3 把同类风险转移到迁移脚本的可逆性声明上(§13.4 L1283、L1255 允许'不可逆则恢复快照'),若声明与实际不符,回退 active 指针本身会制造状态分叉——裁决文本未给出对应的校验机制。"
  ],
  "role": "EMPIRICAL",
  "stance": "以实证裁决:R3 是被 Android/ChromeOS A/B、NixOS 代际切换、Firefox staging、K8s 滚动更新与 Kafka epoch fencing 反复验证过的成熟模式的单机收敛组合,不属于无先例的激进设计;真正超出主流先例的只有\"隔离只读副本验证\"这一严格化环节,以及相对 Erlang/OTP 更宽松地放弃了内存连续性。"
}
```

### 【FOR】

```json
{
  "role": "FOR",
  "stance": "L0 独立控制面以 generation 为粒度的原子升级回滚机制,是唯一能在「单用户本地优先、长期自治 Agent」约束下同时满足崩溃可恢复、插件热替换和阶段一可用阶段二演进三个目标的自洽方案。",
  "key_arguments": [
    "L0 不依赖 L2 即可完成回滚,消除了升级过程中控制面与业务层的循环依赖——当 L2 新版本崩溃时,L0 仍保有独立于 L2 状态的指针切换能力,这是崩溃可恢复性的硬性前提(§2.2 状态归属将升级控制状态明确划归 L0,使其在 L2 故障域之外)。",
    "以完整 generation 为升级单位而非增量补丁,将「语义兼容性验证」从运行时推断前移到部署时校验,使 validating 阶段可以在隔离副本上完成确定性检查而非在生产路径上赌博式试探(§13.4 将此定义为先校验后切换的强制序列)。",
    "单写者租约 + 原子切换 active 指针构成一个无锁的 consensus 原语,在单用户场景下退化为精确的单点序列化,避免了分布式 consensus 的开销同时保留了形式正确性(§13.7 将此列为 Upgrade Manager 的核心职责)。",
    "validating 阶段「禁止真实外部副作用」是 outcome_unknown 语义的唯一来源管控点——任何无法判断的外部副作用在此阶段即被标记而非泄漏到生产路径,这直接保障了长期自治 Agent 的行为可审计性(§17 第 1516-1557 行裁决原文)。",
    "L0 的最小化(不承载业务逻辑)使其在阶段一可作为 thin CLI 二进制交付(§14.1),阶段二演进为完整 OS 时仅扩展能力而不改变接口契约,两阶段之间的架构连续性由 L0 的稳定性保证而非重写实现。",
    "draining 阶段保留旧代际回滚材料而非立即删除,与 Erlang/OTP 的 appup/release_handler 模式一致:旧版本在确认新版本稳定前始终保持可恢复状态,这对插件热替换(§2.1 五层边界中 L2 插件层需独立生命周期)是不可或缺的安全网。"
  ],
  "evidence": [
    "§2.1 五层运行时边界(正文):将 L0 定位为最底层独立控制面,L2 为业务逻辑层,二者故障域隔离是升级回滚不依赖 L2 的架构基础。",
    "§2.2 状态归属(正文):升级控制状态(版本目录、active 指针、租约)划归 L0 所有,L2 状态在 generation 切换时通过快照/检查点机制恢复,实现状态归属的清晰边界。",
    "§13.4 Runtime Core 的升级(正文):定义 generation 为完整升级单位、先校验后切换的强制序列,以及语义连续性优先于内存连续性的原则。",
    "§13.7 Upgrade Manager 的职责(正文):单写者租约、原子切换 active 指针、draining 阶段保留回滚材料的具体职责定义。",
    "§14.1 CLI 运行控制面(正文):阶段一 L0 作为 thin CLI 二进制交付,证明最小化 L0 的阶段一可行性。",
    "§17 第 1516-1557 行:R3 裁决全文,包含 outcome_unknown 的定义和 validating 阶段禁止真实外部副作用的约束。",
    "Erlang/OTP release_handler:以 release(完整代际)为升级单位,旧 release 在 appup 脚本控制下保持 draining 状态直至新 release 确认稳定,与 R3 的 generation + draining 模式同构。",
    "Kubernetes RollingUpdate + ReadinessGate:Pod 分代升级、Readiness 检查通过后才接管流量、旧 Pod 保持 running 直至新 Pod 稳定,与 validating + 单写者租约 + draining 三阶段对应。",
    "Chromium 组件更新:独立 updater 进程(不依赖浏览器主进程)在后台下载安装到独立版本目录,通过注册表指针原子切换,浏览器崩溃不影响 updater 的回滚能力,与 L0 独立控制面模式一致。",
    "VS Code 扩展热替换:Extension Host 独立于主进程运行,扩展更新在隔离环境中验证后原子切换,旧版本保留在 storage 中可回退,与 generation 隔离验证 + 保留回滚材料模式一致。",
    "systemd 的 core:PID 1 作为最小独立控制面,不承载业务逻辑,服务更新通过独立目录 + symlink 原子切换实现,即使所有用户态服务崩溃 PID 1 仍可执行回滚,是 L0 独立于 L2 的直接先例。"
  ],
  "conceded_risks": [
    "generation 粒度升级意味着每次升级必须部署完整 Runtime Core 副本,在存储空间受限的嵌入式设备上可能不可接受——反驳方若能证明 BoenMind 的目标设备存在 <100MB 可用存储的硬约束,此风险将构成实质性威胁。",
    "validating 阶段「禁止真实外部副作用」的判定边界模糊:一个插件在验证期间打开文件句柄但不写入,是否算外部副作用?这个灰色地带可能导致 validating 阶段覆盖率不足,使某些 bug 仅在生产切换后才暴露。",
    "L0 作为阶段一 thin CLI 二进制,其接口契约在阶段二演进为完整 OS 时是否真正不变,存在不确定性——历史上从 CLI 工具演进为 OS(如 Docker → containerd)往往伴随接口重构,R3 假设的架构连续性可能过于乐观。"
  ]
}
```

### 【AGAINST】

```json
{
  "conceded_risks": [
    "单体 Runtime 崩溃或升级失败时，若没有独立恢复入口，用户可能同时失去业务服务、诊断能力和回滚能力。",
    "并发写入权若没有明确租约或栅栏，双代际确实会造成重复事件、状态损坏和不可审计的竞态。",
    "将外部副作用标成 outcome_unknown 并禁止自动重放，虽然降低重复执行风险，却会把核验和用户裁定压力推迟到故障时刻。"
  ],
  "evidence": [
    "基线 §2.1 第95行允许阶段一先做成一个跨平台软件中的逻辑模块或少量进程，只有真实运行证明需要更强隔离时才拆出 L0-L5；这削弱了阶段一立即引入完整双代际控制面的必要性。",
    "基线 §2.2 第143-157行要求 L0 只保存最小控制状态，但同时要求它管理 generation、写入权、排空和恢复；判断迁移是否安全、请求是否可转交，实际上需要理解 L1 合同、L2 状态和 Operation 语义，存在“最小控制面”与“足够安全决策”之间的边界冲突。",
    "基线 §13.4 第1223-1283行把校验、隔离副本恢复、单写者租约、原子切换和回滚放在一条事务链上，但第1254-1256行承认不可逆迁移只能依赖迁移前快照或兼容读取层，未解决切换后新增写入如何合并回旧代际的问题。",
    "基线 §13.3 第1203-1219行明确邮件发送、下单等副作用在重启后可能只能得到 outcome_unknown；因此“active 指针原子切换”只能原子化本地事实，不能原子化本地提交与外部系统提交。",
    "基线 §13.7 第1310-1323行赋予 L0 快照、健康检查、写者约束、长连接交接和回滚等职责；这些职责已经远超启动底座，升级管理器会成为新的高权限状态机和审计关键点。",
    "Kubernetes Deployment 的滚动升级依赖 readiness、progress deadline 和 rollout history，官方文档同时把数据库模式迁移列为应用自身需要处理的兼容问题，而不是把 Deployment 回滚当作数据库回滚（https://kubernetes.io/docs/concepts/workloads/controllers/deployment/）。",
    "PostgreSQL 的 `pg_upgrade` 要求升级前备份并提供失败后的旧集群保留路径，旧集群与新集群的数据目录不能被一个“active 指针”安全地当作任意可逆状态（https://www.postgresql.org/docs/current/pgupgrade.html）。",
    "Erlang/OTP 的 `release_handler` 支持升级和降级，但要求显式 release 配置、版本转换模块和可逆的代码/状态变换；这说明 generation 回滚不是通用原子能力，而是每个状态迁移合同的证明负担（https://www.erlang.org/doc/apps/sasl/release_handler.html）。"
  ],
  "key_arguments": [
    "故障剧本：v1 在快照检查点后收到“发送邮件”请求并已把 Operation 写成执行中，Provider 已向 SMTP 发出数据但 ACK 在切换窗口丢失，v2 验证副本通过后取得租约并恢复该 Operation，用户随后得到一封重复邮件或持续的 outcome_unknown，而回退 active 指针既撤不回 SMTP 行为也不能安全恢复唯一账本。",
    "“原子切换 active”只保护本地 generation 指针，不保护 L2 持久化提交、Provider 游标、文件写入和远端 API 的联合事务，因此切换后的语义回退在最坏情况下会丢失切换后合法写入，不能被称为真正的 rollback。",
    "L0 若真的足够无知就无法判断迁移、租约、长连接和 Operation 是否可安全排空，若为安全而复制 L1/L2 解析、迁移和审计逻辑又会违反“最小且不承载业务逻辑”，并形成第二个容易漂移的事实源。",
    "对阶段一的跨平台单体而言，双运行时、隔离状态副本、快照、保留旧版本和兼容读取层至少带来接近两倍峰值磁盘/内存占用、显著启动与迁移延迟，以及 Windows 文件锁、macOS 签名/权限和 Linux 服务管理三套差异化故障面；更优替代是保留一个极小 Supervisor，只做签名工件选择、进程拉起、心跳和上一版本启动，Runtime 内采用可回放检查点加 expand-contract 数据迁移，Provider 按 §13.1 局部 draining/原子 binding 替换，只有 L1 Major 才启用维护窗口或双代际。",
    "验证期禁止真实副作用会制造审计盲区：健康检查能证明 v2 可读快照和模拟调用，却不能证明真实凭据、限流、网络超时、文件权限、邮件幂等键或交易服务的响应语义，结果是把最危险的错误推迟到切换后；应增加可验证的外部副作用幂等键、Provider 侧查询/对账合同和升级前生产探针，并把不能证明的迁移标为需人工维护窗口而非自动切换。"
  ],
  "role": "AGAINST",
  "stance": "我反对把“L0 独立控制面加完整 Runtime generation 回滚”作为默认升级机制：它能隔离二进制故障，却无法原子化外部副作用和不可逆状态迁移，并以高昂复杂度制造一个新的高权限故障源。"
}
```

## 第二轮:交叉质证

### 【EMPIRICAL-R2】

```json
{
  "conceded_risks": [
    "状态回滚的单向性与数据分叉风险：active 指针回退仅能重置二进制与只读配置，一旦 v2 在取得租约后发生了新的业务持久化写入（或执行了不可逆 schema 迁移），回滚到 v1 将面临数据丢失、schema 不兼容或账本分叉，R3 基线（§13.4 L1254-1256）未提供跨代际数据合并机制。",
    "隔离只读验证的生产写与外部调用盲区：由于单机无法做 Canary 分流，validating 阶段禁止真实外部副作用（§13.4 L1233）导致新版本的写路径、凭据有效性及三方 API 交互从未在切换前预演，首次真实调用即暴露于生产风险中，使 outcome_unknown 成为常态逃生通道。",
    "语义连续性对长时间自治 Agent 的体验折损：放弃内存连续性使代际升级必须将 in-flight 模型流与回合强制标记为 agent.interrupted（§13.4 L1261），在模型长上下文推理或长时间多步工具调用时，升级频率与任务完成率存在直接工程冲突。"
  ],
  "evidence": [
    "Android A/B (seamless) 更新规范：update_engine 独立于 Android Framework 运行，双 slot 保证引导失败自动回退，但 /data 分区独立保留且不随 slot 回滚（https://source.android.com/docs/core/ota/ab）——实证证明“二进制代际回滚”与“用户数据状态”在工程上必须严格解耦。",
    "PostgreSQL pg_upgrade 与 K8s 部署规范：pg_upgrade 要求升级前全量快照且不可逆，K8s 明确 Deployment 的 rollout undo 不承担数据库 schema 回滚责任（https://kubernetes.io/docs/concepts/workloads/controllers/deployment/）——证明代际指针切换绝非通用的状态回滚方案。",
    "Erlang/OTP release_handler 与 OSGi 模块化实践：OTP 的 code_change 内存热更与 OSGi 动态 bundle 局部热更因状态拓扑死锁、版本碎片化被业界普遍放弃，转向容器/代际重启（https://learnyousomeerlang.com/relups）——证明 AGAINST 提议的 Provider 局部原子替换存在已知工程反例。",
    "Kafka KRaft / Epoch Fencing：以 generation lease 隔离双代际写入权（§13.4 L1260）是成熟的无脑裂机制，但在单机上引入了租约超时与自禁写导致的短暂可用性中断。",
    "BoenMind 架构基线具体条文：§2.1 L119-123（L0 最小控制面故障域隔离）；§2.2 L144-157（状态归属与排空恢复）；§13.1 L1120-1160（Provider 局限性）；§13.3 L1203-1219（外部副作用与 outcome_unknown 判定）；§13.4 L1223-1283（validating 隔离恢复、单写者租约、不可逆迁移限制与 interrupted 语义）；§13.7 L1310-1323（Upgrade Manager 职责边界）；§17 L1516-1557（R3 裁决原文）。"
  ],
  "key_arguments": [
    "【接受 AGAINST 并修正己方】实证界定“二进制可回退”与“状态/副作用不可逆”的断层：FOR 宣称 R3 是崩溃可恢复的自洽方案，但实证系统（PostgreSQL、K8s）表明代际指针切换（active 指针）只能原子化可执行工件与只读配置，根本无法原子化外部副作用和切换后的增量持久化状态。AGAINST 正确指出了切换窗口中丢失 ACK 与切换后新增写入导致无法安全 rollback 的硬伤。必须修正立场：R3 的回滚仅能承诺“冷工件与未变更状态的安全回退”，绝非全量业务状态的魔法撤销。",
    "【驳斥 AGAINST】局部 Provider 动态替换违背模块化演进的历史收敛教训：AGAINST 提议用“极小 Supervisor + Provider 局部 draining/原子 binding”替代代际升级，直接撞上了 OSGi 动态 bundle 与 Erlang appup 细粒度热更的经典陷阱——局部动态交接极易引发隐式状态悬挂、依赖死锁与类加载器泄漏。L0 保持对 L2 的零依赖、以完整 Generation 为升级粒度（§2.1 L119、§13.4 L1223），是 Android update_engine 和 systemd 验证过的最少故障域设计，退回细粒度局部热插拔是反模式。",
    "【驳斥 FOR】validating 禁止外部副作用实质是“用审计盲区推迟故障暴露”：FOR 将 validating 阶段禁止外部副作用（§13.4 L1233）美化为可审计性管控点。但在无 Canary 灰度的单机环境下，新代际未经真实写路径演练即接管租约，把凭据过期、限流及网络超时等最致命的错误全部推迟到生产切换后，并迫使未决事务堆积进 outcome_unknown（§13.3 L1210）。FOR 所谓的“确定性校验”在写路径上覆盖率为零，必须辅以生产探针或幂等核验协议。",
    "【严格与宽松的实证定位】BoenMind 处于极端的工程折中点：相比 Erlang/OTP relup，BoenMind 更宽松——果断放弃内存连续性（§13.4 L1263），将 in-flight 任务标记为 interrupted 并从持久状态冷恢复，成功规避了 OTP 堆栈转换的灾难；相比 Android/NixOS，BoenMind 更严格——在单机上强推“隔离状态副本验证 + 单写者租约”（§13.4 L1231, L1260），以分布式 epoch fencing 的复杂度换取零脏写。这种不对称性决定了它在静态工件升级上高度可靠，但在长程多步 Agent 正在执行复杂外部交互时，代价是频繁的任务中断与单用户对账负担。",
    "【裁决落地的机制收敛要求】L0 控制面与状态迁移契约必须解耦：采纳 AGAINST 提出的架构简化诉求，L0 严禁承载 L2 迁移语义解析，只负责工件解压、SHA256 校验、守护拉起、租约分发与进程回退；状态演进必须强制遵循 expand-contract 双向兼容模式，外部交互必须依赖 Idempotency-Key 与 Provider 补偿对账，不可将数据一致性押注在 active 指针回滚上。"
  ],
  "role": "EMPIRICAL-R2",
  "stance": "以实证裁决（已修正）：R3 的「L0 独立控制面 + Generation 代际指针切换」是 Android A/B、NixOS 与 systemd 验证过的工件升级最佳实践，能彻底隔离二进制故障；但其「切换后异常可回退 active 指针」的承诺在机制上仅适用于只读与未变更状态，在发生持久化新写与不可逆外部副作用时必然失效，必须将二进制回滚与状态 expand-contract 迁移及幂等对账彻底解耦。"
}
```

### 【FOR-R2】

```json
{
  "role": "FOR-R2",
  "stance": "经质证修正：L0 独立控制面以 generation 为粒度的原子升级回滚机制仍然是正确且必要的，但需承认写路径验证盲区是结构性代价而非可修补缺陷，并接受 R3 的'原子性'必须被精确定义为'本地状态指针的原子性'而非'端到端事务的原子性'。",
  "key_arguments": [
    "接受 EMPIRICAL 的写路径验证盲区 critique：validating 阶段禁止真实外部副作用确实意味着 v2 的第一次真实写入从未被演练——但这是单机无 canary 约束下的最优折衷而非设计缺陷。Android A/B 的 dm-verity 校验同样不执行'真实写路径'验证，它依赖的是'启动成功即隐式验证'的后置确认(boot_success 超时)。R3 的正确补强方向是在 §13.7 Upgrade Manager 职责中增加一个显式的 post-switch probation period（参照 Android 的 markBootSuccessful 超时机制），在窗口内检测到异常时自动回退 active 指针，而非在 validating 阶段追求不可能的完整覆盖。",
    "驳斥 AGAINST 的'原子切换只保护本地指针不保护外部副作用'论点：这构成稻草人——R3 裁决原文明确写了'无法判断的外部副作用进入 outcome_unknown'(§17 L1550-1557)，从未声称端到端原子性。AGAINST 的邮件故障剧本(ACK 丢失导致重复)正是 outcome_unknown 要捕获的语义：L0 不假装能撤回 SMTP，它做的是(1)通过单写者租约确保切换窗口内只有一个代际持有写权，消除双写的可能；(2)将窗口边界上的不确定副作用标记为 outcome_unknown 交由用户裁定。这比 AGAINST 提出的'可回放检查点+expand-contract 迁移'更诚实——后者隐含假设所有外部副作用都可幂等重放，这在邮件、支付、IoT 指令等场景下同样不成立。",
    "驳斥 AGAINST 的'L0 最小化与足够安全决策之间的边界冲突'论点：AGAINST 混淆了'理解语义'与'执行语义'。L0 判断迁移是否安全的依据是 L1 合同中声明的迁移声明(migration declaration)和可逆性断言(§13.4 L1254-1256 允许'不可逆则恢复快照')，而非自己解析 L2 状态。这完全类比 systemd 不需要理解 nginx 配置语义就能判断'新配置 reload 失败则回滚旧配置'——systemd 检查的是进程退出码和 notify 就绪信号，而非解析配置文件。L0 是合同的执行者而非合同的编写者，'最小控制面'的边界在于它只执行已声明的迁移合同，不发明迁移策略。",
    "接受 AGAINST 的阶段一复杂度 critique 并修正立场：双运行时+隔离状态副本对阶段一单体确实偏重。修正为：阶段一应将 R3 降级为 L0 Major 升级专用路径(即 §13.4 中 L1 Major 才触发完整 generation 切换)，L2 插件热替换和 L1 Minor 仍走 §13.1 的局部 draining+原子 binding 替换——这与 AGAINST 提出的'只有 L1 Major 才启用维护窗口或双代际'高度一致。但 AGAINST 据此否定整个 R3 机制是错误的：阶段一不需要每次升级都走完整 generation 流程，但阶段二长期自治 Agent 必须具备此能力，提前在阶段一建立骨架(即使简化为'签名选择+进程拉起+上一版本启动')比阶段二从零构建成本低一个数量级。",
    "驳斥 AGAINST 的'Upgrade Manager 成为新高权限故障源'论点并回击 EMPIRICAL 的'租约异常即自禁写'担忧：任何升级系统都需要一个升级控制点，问题不在于是否存在这个控制点，而在于它是否拥有最小必要权限。L0 的 Upgrade Manager 只持有三项能力：(1)读写 active 指针(一个 symlink 或原子文件替换)；(2)授予/撤销单写者租约(一个文件锁或 epoch 计数器)；(3)启动/停止 Runtime 进程。这三项都不涉及业务数据读写。EMPIRICAL 说的'租约异常即自禁写'恰恰是设计意图——当租约持有者崩溃无法续约时，L0 拒绝授予新租约直到手动介入，这比'静默允许新代际在未知状态下接手写入'安全得多。参照 Kafka controller epoch fencing：epoch 冲突时宁可拒绝写入也不允许 split-brain，这是分布式系统经数十年收敛的严格化选择，R3 在单机上采用同一原则是正确的。",
    "接受 EMPIRICAL 的'语义连续代价(in-flight 标记 interrupted)'critique 但重新框定：每次升级中断 in-flight 回合确实是结构性行为，但 EMPIRICAL 将其框定为'连续性承诺的失败'是错误的。长期自治 Agent 的正确连续性模型是'会话级连续'而非'操作级连续'——Agent 的价值在于跨会话保持用户意图和上下文(§2.2 的持久状态恢复)，而非保证单次 LLM 生成流不中断。generation.changed + agent.interrupted 恢复事件(§13.4 L1261)正是这个模型的机制化表达：升级后 Agent 从持久状态恢复上下文并告知用户上次操作被中断，这比假装操作可以无缝延续(如 Erlang relup 的 code_change 经常失败地做的那样)更诚实。"
  ],
  "evidence": [
    "§13.4 L1231-1237：validating 阶段定义(隔离只读恢复、禁止真实外部副作用)——此约束的精确含义是写路径验证盲区是结构性代价，接受此代价是单机无 canary 约束下的最优折衷。",
    "§13.4 L1254-1256：不可逆迁移依赖迁移前快照或兼容读取层——这证明 R3 已预见不可逆场景并给出明确退路(恢复快照)，而非假装所有迁移可逆。",
    "§13.4 L1260-1268：单写者租约 + 生成流标记 interrupted + outcome_unknown——这是 R3 对外部副作用不可原子化的诚实承认，contra AGAINST 的稻草人指控。",
    "§13.7 L1310-1323：Upgrade Manager 职责清单——快照、健康检查、写者约束、长连接交接、回滚，但不含业务数据读写或语义解析，contra AGAINST 的'高权限状态机'指控。",
    "§14.1 L1398：L2 不可用时 CLI 仍可执行排空/回滚——这是 L0 独立于 L2 的阶段一交付证据。",
    "§2.1 L95：阶段一可先做逻辑模块——接受此条为 AGAINST 阶段一降级论的支撑，但 §2.1 L119-123 的 L0 最小控制面定义说明即使作为逻辑模块，L0 的接口契约仍应在阶段一确立。",
    "Android A/B seamless update + markBootSuccessful 超时自动回退(https://source.android.com/docs/core/ota/ab)：写路径同样在切换前零覆盖，依赖后置确认——R3 应对标此机制增加 post-switch probation period。",
    "Kafka controller epoch fencing(https://developers.redhat.com/articles/2025/09/17/deep-dive-apache-kafkas-kraft-protocol)：epoch 冲突时拒绝写入优于 split-brain——租约异常自禁写是同一原则的单机同构。",
    "systemd notify 机制：PID 1 不解析业务配置语义，仅检查进程退出码和 sd_notify 就绪信号——L0 执行迁移合同而非理解迁移语义的直接先例。",
    "NixOS generation rollback(https://nixos.wiki/wiki/Overview_of_the_NixOS_linux_distribution)：任意 generation 可回退但外部状态(数据库)需独立处理——证明'本地指针原子性≠端到端原子性'是全行业共识，R3 用 outcome_unknown 显式标注这一边界是加分项而非减分项。"
  ],
  "conceded_risks": [
    "validating 阶段写路径零覆盖是结构性代价而非可修补缺陷——即使增加 post-switch probation period 也只能缩短暴露窗口，无法消除'v2 第一次真实写入在切换后'这一事实(接受 EMPIRICAL 此点)。",
    "阶段一完整双代际控制面对单体软件偏重——应将完整 generation 流程限定为 L1 Major 升级专用，L2 插件和 L1 Minor 走局部替换(接受 AGAINST 此点并修正立场)。",
    "outcome_unknown 标记将核验压力推迟到故障时刻——用户在故障时需要理解哪些操作可能重复、哪些已丢失，这对非技术用户构成认知负担(接受 AGAINST 和 EMPIRICAL 双方此点)。",
    "post-switch probation period 的健康判据尚未在基线中定义——需要补充具体的 health check 合同(如：新代际启动后 N 秒内完成一次成功的 Agent 会话恢复、Provider binding 确认、无未处理 error event)，否则自动回退无法触发(与 EMPIRICAL 的'缺少异常判定标准' critique 一致)。"
  ]
}
```

### 【AGAINST-R2】

```json
{
  "conceded_risks": [
    "我接受 EMPIRICAL 关于“独立版本目录、校验、原子指针切换、保留旧版本”已有成熟先例的判断；Android A/B、NixOS generation 和 Firefox staging 证明二进制工件切换本身不是激进设计。我的立场因此修正为：L0 薄控制面应保留，但完整 Runtime generation 双实例与自动回滚不应成为所有升级的默认路径。",
    "我接受 FOR 关于 L0 不应依赖待升级 L2 才能恢复的核心判断。若 L0 与 L2 同进程，L2 的崩溃、死锁或损坏确实可能同时夺走诊断、启动和回滚入口；但这只证明需要独立启动器、指针和健康状态，不证明 L0 必须理解或执行 L1/L2 的迁移与语义判断。",
    "我仍承认单写者约束、代际 fencing、旧版本保留和 outcome_unknown 是必要的可靠性构件。没有写者栅栏时，旧代际延迟写入与新代际恢复写入可并发落盘，后果会从重复副作用升级为账本分叉和不可审计。",
    "我仍承认阶段一单体部署中，独立进程会增加内存、磁盘、启动时间、跨平台文件锁和服务管理的成本；但把这些成本绑定到每次升级，未必比一次性维护窗口或按风险启用双代际更划算。"
  ],
  "evidence": [
    "“原子 active 指针”只能原子化 L0 对版本选择的本地记录，不能原子化 L2 状态提交、Provider 游标、文件系统写入和远端 API 提交。基线 §13.3 第1203-1219行已承认外部副作用在重启后可能只能得到 outcome_unknown；因此 Android/NixOS 的工件回退先例不能推出业务语义可回退。",
    "FOR 将单写者租约称为“无锁的 consensus 原语”并不成立。单机租约只能提供排他写入和过期 fencing，不能证明旧代际的已发出请求、远端响应或延迟本地写入已经被观察；若 v1 在失去租约前已向 SMTP 发包，v2 只能阻止后续本地写，无法撤销或确定远端结果。应称其为 lease/fencing，而非 consensus。",
    "validating 只读恢复能验证快照可解析、迁移脚本可运行和部分确定性读取，但不能验证真实凭据、网络超时、限流、文件权限、远端幂等行为或“请求已提交但 ACK 丢失”的状态。基线 §13.4 第1231-1237行的“禁止真实外部副作用”恰好使最危险的生产路径未被覆盖；这不是确定性增强，而是明确的审计盲区。",
    "具体故障剧本：v1 持有写者租约，将邮件 Operation 持久化为 executing，随后向 SMTP 服务端发送 DATA；网络在服务端接收后、v1 获得 ACK 前断开。L0 认为 v1 不健康，切换到已通过 validating 的 v2；v2 从快照恢复 executing 并重试，收件人收到两封邮件。若回退 active 到 v1，无法撤销第二封邮件，且切换后的 Operation、Provider 游标和快照之间没有通用合并规则，所谓 rollback 只能回退启动入口，不能回退事实。",
    "基线 §13.4 第1254-1256行把不可逆迁移的安全性寄托于迁移前快照或兼容读取层，却没有定义切换后新写入如何反向投影到旧代际。若 v2 已采用新索引、压缩格式或新增状态字段，回退 active 指针可能得到“旧代码读取新事实”的静默丢字段、重复消费或错误默认值；这比直接保持旧版本服务更危险。",
    "更优替代是分层升级策略：L0 仅负责签名工件选择、独立拉起、心跳、租约/fencing、active 记录和上一版本启动；默认采用 expand-contract 兼容数据格式与 append-only 检查点，先让单一 Runtime 完成可回退的 schema 扩展，再在维护窗口或具备幂等查询/对账合同的 Provider 上切换。只有涉及不可兼容 L1/L2 语义且能证明快照、迁移和外部副作用合同的版本，才启用完整双代际 validating；无法证明的升级标为需人工维护窗口，而不是宣称可自动回滚。"
  ],
  "key_arguments": [
    "我接受“L0 独立控制面”和“工件级 generation 切换”两个局部论点，但驳斥“因此完整 generation 回滚是唯一自洽方案”：独立启动与版本指针只解决控制入口存活，不能解决不可逆状态和外部副作用的联合原子性；应将双代际降为按风险启用的升级模式。",
    "我驳斥“单写者租约等于 consensus”。租约可以 fencing 本地旧写者，却不能确认远端副作用是否已提交，也不能让切换前后两个持久状态副本自动合并；因此它是必要的排他机制，不是足以支撑语义回滚的证明。",
    "我驳斥“禁止副作用使 validating 更可审计”。它确实避免验证期污染生产，但同时保证不了真实 Provider 的响应语义，尤其覆盖不了提交成功而 ACK 丢失的窗口；必须配套外部幂等键、查询/对账接口和升级前生产探针，否则 validating 通过不应成为自动切换依据。",
    "我接受对方援引 Android、NixOS、Firefox、Kubernetes 的实证，但指出这些先例主要证明可执行文件或流量承载的回退，不证明数据库、Operation 日志和远端副作用可回退。Kubernetes 官方也将数据库模式兼容留给应用处理；这反而支持把状态迁移合同从 L0 控制面中分离出来。",
    "我的修正方案是“薄 L0 默认 + 兼容迁移默认 + 高风险代际切换例外”：L0 不解析业务语义；Runtime 通过版本化迁移合同、append-only 检查点、Provider 幂等/查询/对账能力给出可验证前置条件；不满足条件的升级进入维护窗口并保留旧版本，而不是自动宣称 rollback。"
  ],
  "role": "AGAINST-R2",
  "stance": "已修正：我支持独立薄 L0 与工件级代际切换，但反对把缺乏外部副作用证明和状态反向迁移合同的完整 Runtime generation 自动回滚设为默认机制。"
}
```

## 合成裁决

```json
{
  "adr_points": [
    "采用 L0 作为独立于待升级 Runtime 的最小控制面:其持久状态仅限 active 指针、安装版本、升级事务与回滚指针;能力仅限工件选择校验、指针切换、租约/fencing、进程拉起停止与上一版本启动;L0 不解析 L1 合同、L2 状态或迁移脚本语义,仅消费迁移脚本按 §13.4 声明的可验证前置条件(可逆性断言、失败清理方式、退出码、健康信号),回滚入口不依赖 L2 存活(§13.7、§14.1)。",
    "Runtime Core 以完整 generation 为升级单位,执行独立版本目录安装→校验→快照/检查点→隔离 validating(只读状态副本、禁止真实外部副作用)→取得单写者租约→原子切换 active→旧代际 draining 并保留回滚材料的强制序列;完整双代际流程保留给不可兼容 L1/L2 语义升级(L1 Major/Runtime Service 级),Patch/Minor 与 L2 插件默认走局部替换路径,且局部路径必须满足同等的单写者 fencing 与排空保证(§13.1、§13.5、§13.6)。",
    "明确原子性边界:generation 切换与指针回退只原子化可执行工件、只读配置与本地指针,不原子化持久状态新写入、Provider 游标与远端 API 提交;二进制回滚与状态演进了耦,状态迁移强制遵循版本化迁移合同与 expand-contract 兼容策略,数据一致性不得押注在指针回滚上。",
    "回退安全性前置条件:仅当状态格式未发生不兼容变更(或存在声明的兼容读取层)且切换后未产生状态分叉时,才允许自动回退 active 指针;不可逆迁移只能经迁移前快照恢复;状态已分叉时禁止指针级自动回退,转入快照恢复或人工维护窗口,禁止制造旧代码读新事实的静默损坏。",
    "新代际切换后设置 post-switch probation 观察窗并定义健康合同:判据至少包含规定时限内完成一次成功的 Agent 会话恢复、Provider binding 确认、租约正常续约、无未处理 error event;判据全部满足视为升级成功,状态未分叉的异常触发自动回退;健康判据未在基线定义并实现前,不得启用任何自动回滚。",
    "外部副作用治理:触及真实 Provider 的升级,validating 通过不构成自动切换的充分条件,须叠加幂等键、Provider 查询/对账合同或升级前生产探针中的至少一项作为前置;不可证明副作用的升级标为人工维护窗口并保留旧版本,不得宣称自动回滚;outcome_unknown 禁止自动重放,先核验外部系统,无法确认时交用户裁定,核验清单作为升级恢复的强制交付项。",
    "单写者租约定位为 fencing 机制而非 consensus:保证切换窗口内唯一代际持写权、旧代际禁写,不提供远端副作用确认或撤销能力;租约异常或无法续约时拒绝授予新租约并冻结写入等待人工介入,宁可拒写不可双写。",
    "连续性模型:升级目标是语义连续与会话级连续,不是内存连续;in-flight 回合标记 agent.interrupted 并从持久状态恢复,升级后发布 generation.changed 与恢复事件;升级频率纳入策略约束(窗口合并、最低升级间隔、用户可控的升级计划),连续性承诺不得依赖单次生成流不中断。"
  ],
  "conditions": [
    "L0 保持最小能力集——active 指针读写、单写者租约授予/撤销、Runtime 进程启停、工件校验与上一版本启动,禁止承载 L1/L2 迁移语义解析或业务数据读写;迁移安全性判断只能来自声明的可验证前置条件,不得由 L0 自行解释。",
    "active 指针自动回退仅限两个前提同时成立:状态格式未发生不兼容变更(或存在声明且验证过的兼容读取层),且切换后未产生跨代际状态分叉(新代际尚未执行不可逆迁移或产生新业务写入);不满足时只能经迁移前快照恢复或人工维护窗口回退。",
    "完整双代际 generation 流程仅用于不可兼容语义(L1 Major/Runtime Service 级)升级;L1 Minor 与 L2 插件默认走 §13.1/§13.6 局部 draining/原子 binding 路径,且该局部路径必须满足与 generation 路径同等的单写者 fencing 与排空保证方可承载。",
    "自动回滚启用前必须在基线中补充定义健康合同(probation 判据及其检测点);判据未定义期间,切换后回退仅为人工操作,不宣称自动回滚。",
    "触及真实 Provider 的升级,自动切换前置必须包含副作用可证明性(幂等键、Provider 查询/对账合同、升级前生产探针至少其一);不可证明者标记人工维护窗口并保留旧版本。",
    "outcome_unknown 的处置协议(禁止自动重放、外部系统核验流程、用户裁定入口)与升级频率约束(合并窗口/最低间隔)须在阶段二自治化之前落地,否则连续性承诺对外不得宣称。"
  ],
  "consensus_points": [
    "L0 最小独立控制面的必要性与能力集已三方收敛:L0 必须独立于待升级的 L1/L2 保有回滚入口(基线 §13.7 L1323、§14.1 L1398 已明确),其能力限于签名工件选择与完整性校验、active 指针读写、单写者租约/fencing 授予与撤销、Runtime 进程拉起/停止、上一版本启动;L0 不解析 L1 合同、L2 状态或迁移脚本语义,只消费显式声明且可验证的前置条件(迁移可逆性断言、退出码、就绪信号、健康事件)。FOR-R2 的『合同执行者而非编写者』定位与 AGAINST-R2 的薄 L0 职责清单在第二轮实质收敛,EMPIRICAL-R2 的机制收敛要求与之相同。",
    "工件级 generation 切换模式不属激进设计:独立版本目录安装→校验→原子切换 active→旧代际保留为回滚材料,与 Android A/B slot、NixOS generation、Firefox staging 逐一同构,AGAINST-R2 已明确接受此局部论点;但该原子性的覆盖边界被三方共同确认为:仅原子化可执行工件、只读配置与本地指针,不原子化持久状态新写入、Provider 游标与远端 API 提交(Android /data 分区不随 slot 回滚、K8s rollout undo 不承担 schema 回滚为共同实证)。",
    "单写者租约是必要的排他写机制:防止双代际并发写入导致重复事件与账本分叉(基线 §13.4 L1260、§2.2 L157),三方均接受其必要性;经 AGAINST-R2 质证,其保证范围限定为本地写权排他与旧代际禁写,性质是 lease/fencing 而非 consensus(FOR 的『无锁 consensus 原语』表述未被再主张,视为撤回),不提供远端副作用确认或撤销能力;租约异常时拒绝授予新租约而非静默放行(拒写优于 split-brain)为三方共同接受的设计取向。",
    "二进制回滚与状态演进了耦是硬性结论:状态迁移必须遵循版本化迁移合同(§13.4 L1283 已要求声明可逆性)并优先 expand-contract 兼容策略;active 指针回退仅在状态格式未变或存在声明兼容读取层时安全;切换后新写入无法自动反向合并进旧代际——此缺口经 AGAINST 两轮主张后未被任何一方成功驳倒,FOR-R2 与 EMPIRICAL-R2 均已明确承认,构成对裁决原文『切换后异常可回退 active 指针』无边界表述的有效限定。",
    "隔离 validating 的结构性盲区:只读状态副本恢复可验证快照可解析、迁移脚本可运行与确定性读取,但结构上无法覆盖真实写路径、凭据有效性、限流、网络超时与『已提交未确认』窗口;单机无 canary 分流,新代际首次真实写必然发生在切换后。FOR-R2 已承认这是『结构性代价而非可修补缺陷』,EMPIRICAL 两轮一致,AGAINST 亦不否认其防污染价值,仅主张需补充机制。",
    "outcome_unknown 是外部副作用不可原子化的诚实边界标记,而非缺陷:FOR-R2 指出 R3 从未宣称端到端原子性、AGAINST 的邮件故障剧本正是 outcome_unknown 要捕获的语义——此反驳成立;同时三方共同承认其代价:核验与裁定压力推迟到故障时刻、对非技术用户构成认知负担,须以核验协议(先查询外部系统再决策,§13.3 L1213-1219 已有雏形)与披露机制缓解,禁止自动重放。",
    "分层升级策略(完整流程非默认路径):完整 generation 升级流程保留给涉及不可兼容 L1/L2 语义的升级(L1 Major,对应基线 §13.5 L1294-1295『Major 必须经过迁移、维护窗口或明确的兼容桥』与 §13.6 对照表的既有分层);L1 Minor 与 L2 插件默认走 §13.1/§13.6 局部 draining/原子 binding。FOR-R2 与 AGAINST-R2 第二轮明确收敛(且该分层本就是基线既有内容),EMPIRICAL-R2 反对的仅是『以局部热替换全面取代代际升级』这一更强的主张,不构成对该分层本身的驳倒。"
  ],
  "disputes": [
    "自动回滚的触发判据与自动/人工边界仍未收敛:FOR-R2 主张对标 Android markBootSuccessful 增加 post-switch probation 窗口、窗口内检测到异常即自动回退;AGAINST-R2 主张状态分叉后自动回退『比保持旧版本更危险』,无法证明副作用合同的升级应转人工维护窗口而非自动回滚。健康判据的具体构成(FOR-R2 自己承认基线未定义)『状态未分叉』由谁在何时检查、以及分叉边界情形下自动与人工的精确分界,两轮均未给出可实现的定义。",
    "自动切换的前置充分性未收敛:AGAINST-R2 与 EMPIRICAL-R2 主张触及真实 Provider 的升级须具备幂等键、查询/对账合同或升级前生产探针方可自动切换,否则 validating 通过只作参考;FOR-R2 主张 validating+probation 已是单机约束下的最优门槛,未接受将副作用合同设为强制前置。FOR 的 probation 价值(故障检测窗口)与 AGAINST 的合同价值(故障预防)互补且互相未被驳倒,但『自动切换门槛由哪些必要条件构成』仍是活分歧。",
    "连续性承诺的充分性未收敛:EMPIRICAL 坚持升级频率与 interrupted 语义相乘对长程多步任务完成率构成直接工程冲突,隐含要求升级频率约束或合并窗口等策略机制;FOR-R2 将连续性重框为『会话级连续』并主张 interrupted+持久状态恢复已是诚实模型,但未给出任何频率约束机制;AGAINST 对 draining 只保护不可重复收尾事务的宽松度质疑在 R2 未再展开但未被回应。",
    "局部替换路径的风险评级未收敛:EMPIRICAL-R2 援引 OSGi 动态 bundle 与 Erlang appup 细粒度热更的失败史,认为局部 draining/原子 binding 存在隐式状态悬挂、依赖死锁与类加载器泄漏风险;而 FOR-R2 与 AGAINST-R2 的分层共识恰恰依赖基线 §13.1/§13.6 的该路径承载 Minor/插件升级。分层方案成立的前提——局部路径具备与 generation 路径同等的 fencing/排空/崩溃隔离保证——未获三方一致确认,EMPIRICAL 的警告未被 FOR/AGAINST 正面回应。"
  ],
  "amendments": [
    "将『切换后异常可回退 active 指针』修订为带边界的表述:『active 指针回退仅对可执行工件、只读配置与本地指针原子有效;仅当状态格式未发生不兼容变更(或存在声明兼容读取层)且切换后未产生状态分叉时方可自动回退;不可逆迁移已执行或状态已分叉时,只能经迁移前快照恢复或人工维护窗口回退,禁止以指针回退制造旧代码读新事实的静默损坏(细化 §13.4 L1250-1258)。』",
    "新增 post-switch probation 条款:新代际切换后进入观察窗,健康判据至少包括——规定时限内完成一次成功的 Agent 会话恢复、Provider binding 确认、租约正常续约、无未处理 error event;判据满足视为升级成功(对标 Android markBootSuccessful),状态未分叉的异常触发自动回退,状态已分叉的异常冻结写入并转人工处置;判据未在基线定义前不得启用自动回滚。",
    "新增分层适用条款:『完整 generation 升级流程(双代际+隔离 validating)适用于不可兼容 L1/L2 语义升级(L1 Major)与 Runtime Service 级升级;L1 Minor 与 L2 插件默认走 §13.1/§13.6 局部 draining/原子 binding 替换,且局部路径强制同等单写者 fencing 与排空保证』——将 §13.5/§13.6 既有分级显式写入 R3 裁决文本。",
    "新增外部副作用前置条款:『触及真实 Provider 的升级,validating 通过不构成自动切换的充分条件;须叠加幂等键、Provider 查询/对账合同或升级前生产探针中的至少一项;不可证明副作用的升级标记为人工维护窗口并保留旧版本,不得宣称自动回滚』——将 §13.3 的核验流程前置为升级门槛。",
    "术语修正:全文将『单写者租约』统一定位为 lease/fencing 机制(排他写权+过期栅栏),不采用『consensus 原语』表述;明确其保证范围限于本地写权排他与旧代际禁写,不提供远端副作用确认或撤销;租约异常时拒绝授予新租约并冻结写入等待人工介入。",
    "明确语义连续边界与频率约束:『升级目标是会话级连续;in-flight 回合标记 agent.interrupted 并自持久状态恢复;升级频率纳入策略约束(升级窗口合并、最低升级间隔),连续性承诺不依赖内存连续,且 draining 范围与 interrupted 宽松度不得随实现放宽。』"
  ],
  "ruling_id": "R3",
  "verdict": "amend"
}
```
