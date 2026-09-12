---
status: accepted
date: 2026-09-12
summary: 配置文件 JSON 读写原语单源至 bm-core,严格/宽容两种损坏策略显式命名,消除三份实现与同文件双策略
supersedes: []
superseded_by: []
---

# ADR-0052: 配置文件 JSON 读写原语单源(bm-core::json_store)

- 关联: ADR-0047(三处局部去重单源)、ADR-0038(规则合同化/解释器)、ADR-0023(墓碑文件)、ADR-0012(配置文件口径)
- 背景(2026-09-12 架构评估,issue #71): 「读一个 config JSON」在仓内有**三份实现**,且损坏策略散落:
  1. `bm-core` 内多处手写 `read_to_string().ok()? + from_str().ok()?`(`roles.rs` 两处、`workspace.rs`、`limits.rs`、`context_log.rs`);
  2. surface 的 `webadmin::json_store`(issue #38 曾收口 providers/skills/roles/mcp 四域,但**仅覆盖 surface**);
  3. surface 的 `config_store::{read_file_strict,read_file}`。
  后果已经可见:`config/roles.json` 被 webadmin(损坏→拒绝)与 `bm-core::roles`(损坏→静默 None)**两套策略各解析一次**;`webadmin/mcp/lifecycle.rs` 内第三种手写习语(原 `read_tombstones`/`upsert_tombstone`/`remove_tombstone` 三处)。策略本身是正确的(写前严格、只读宽容),但**没有名字**,于是无法复用、只能重抄。

## 决策

**把「配置文件 JSON 读写」收口为 `bm_core::json_store` 单一实现,策略显式命名。**

1. **一份实现**:`read_json_file`(严格)/`read_json_lenient`(宽容)/`write_json_file`/`crlf`。置于 `bm-core`——`atomic_write` 已在此(`ports::persist`),配置读写与落盘工具同层,且 core 的只读消费面(roles/workspace/limits)要能用,不能被 surface 私有原语反向服务。
2. **策略是一等命名,不是参数开关**:
   - `read_json_file` = **严格**:仅 NotFound 为 `Missing`,其余 IO 错误与 JSON 损坏一律 `Err`。用于**写入前置读取**(损坏必须可见,否则下次保存整库覆写 → 数据丢失)。
   - `read_json_lenient` = **宽容**:缺/坏一律 `None`。用于**只读消费**(读不到回退默认)。doc 明写「不得用于写入前置读取」。
   两种策略**刻意保留**,本 ADR 不是要抹平差异,而是让差异有名字、可审计。
3. **写法**:strict 的错误文案仍由调用方传域前缀(保持逐字不变),原语不内置业务措辞。
4. **旧路径不散改**:surface 保留 `webadmin::json_store` 作为**纯 re-export 路径别名**(无第二份实现);`config_store::crlf` 同法 re-export。与 bm-persist re-export `atomic_write` 的手法一致。

## 后果

- **同类三份实现归零**:`grep read_to_string` 在生产配置面只剩 `bm_core::json_store` 本体;新增配置域不再重抄样板,只选策略。
- **同文件双策略消除**:`roles.json` 现在读写两侧都经同一原语(webadmin 写用 strict,core 读用 lenient,策略显式且各得其名)。
- **零行为变更**:所有调用点的策略与错误文案逐条保持(严格点仍严格、宽容点仍宽容);净删约 57 行;全量测试通过、clippy 零警告。
- **未做(如实标注)**:①**不统一为单一策略**——strict/lenient 对应「写前」与「只读」两种真实语义,强行统一会要么丢数据、要么阻塞主流程;②`token.rs`(明文令牌 trim)与各 `.jsonl` 追加日志(事件/执行/上下文日志)非配置 JSON,不走本原语;③`webadmin::json_store` 别名保留而非删除,是为了让调用点零散改(纯 re-export,无实现重复)。
