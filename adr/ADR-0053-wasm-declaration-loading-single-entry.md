---
status: accepted
date: 2026-09-12
summary: wasm 家族声明装载单入口——路径/形状/选择规则收口 bm-core,启动装配与整表重载共用一条装载路径
supersedes: []
superseded_by: []
---

# ADR-0053: wasm 家族声明装载单入口 
- 关联: ADR-0049(wasm 声明格式合一/manifest 合成)、ADR-0051(ManifestSpec 全族合成单源)、ADR-0041(通用 wasm 宿主)、ADR-0016(技能脚本执行面)、ADR-0046(端口反转,本为其延伸)、issue #68 - 背景(): ADR-0049 统一了 manifest **合成**,ADR-0051 统一了**缺省集**,但「读哪个文件、什么形状、哪些条目算声明、技能根目录在哪」这套**装载知识**仍分散:bootstrap 侧 `boenmind-server.rs` 有两个自由函数(`register_skill_scripts`/`register_wasm_plugins`)各自 `read_to_string + serde_json::from_str` 解析 `skills.json` 形状;管理面 `webadmin/skills.rs`、`webadmin/plugins.rs` 又各持一份。装载路径计数:代码汇编 + 4 个自由函数 + 2 个管理面读函数 = **6 条**(评估报告口径),同一形状被解析多遍。 
## 决策 
**wasm 家族声明的「路径 + 形状 + 选择规则」收口到 `bm_core::ports::skill_host`,装载收敛为单一函数。** 
1. 新增纯读取器(单源):`skills_config_path` / `plugins_config_path` / `skill_root`(路径约定单源)、`read_skill_definitions`(读 `skills.json`,只取声明 `scripts` 的技能,缺/坏 → 空表)。 2. 新增装载单入口 `load_wasm_declarations(host, data_dir)`:技能(带 scripts 者)在先、通用插件(整表声明文件)其后,合成为待注册能力对。bootstrap 的 `load_skill_scripts` 改为**只调它**,两个自由函数删除。 3. **粒度差异是语义,不合并**:整体装载(启动 / `/admin/plugins` 整表重载)走 `load_wasm_declarations` 或其插件半边;**单个粒度**的增删改仍走 `SkillHost::register_skill` / `unregister_skill` / `unregister_all_generic`——单技能改动不该重编译全表,这是刻意保留。 4. **管理面 CRUD 读取不算装载**:**保留** `webadmin/skills.rs::read_skills`(原始 JSON,供前端表单回显)与 `plugins.rs::read_plugins`——它们要的是磁盘原样文档而非归一化声明,强行改走装载读取器会丢掉展示所需字段。此为刻意的边界,非常遗漏。 
## 后果 
- **装载单入口**:bootstrap 不再自带 `skills.json` 形状解析;`grep register_skill_scripts` 归零。新增 wasm 声明源(若将来出现)只需在 `load_wasm_declarations` 加一条,不必再改两处装配。 - **读取/选择规则单源**:「哪些技能算能力来源(有 scripts)」「技能根目录在哪」在 core 一处定义,启动与热重载不可能再漂移。 - **零行为变更**:读取的宽容度(缺/坏 → 空表)与旧双侧实现一致;技能/插件的装载顺序、日志措辞保留;全量 76 组测试通过、clippy 零警告。 - **未做(如实标注)**:①**磁盘格式不合并**(`skills.json` vs `plugins.json` 各保形状)——ADR-0049 已裁决,内核内部归一化即可;②**未合并单粒度与整体装载**——语义不同(见决策 3);③**MCP 声明装载不在本 ADR 范围**(MCP 走 `read_mcp_servers` + `load_mcp_setups` + supervisor,自成一族;其「插件声明通用合同」问题另论)。 