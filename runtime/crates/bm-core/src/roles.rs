//! W4b 角色/技能 prompt 组装:roles.json(角色,含挂载技能列表)+
//! skills.json(技能知识包;合同 capability/skill.v0_1)。
//! 组装规则:激活(或指定)角色的 system_prompt 为基底,其挂载技能的
//! instruction 依次追加。Skill 只是数据,加载不改变权限(基线定义)。
//! 该函数是角色 prompt 的唯一组装点:回合组装(turn.rs)与会话创建
//! (openai_compat)共用,保证两处口径一致。

use std::path::Path;

/// 角色 prompt 的结构化组成(ADR-0056):persona 基底 + 挂载技能指令。
/// 供诊断面(上下文透视)直读,**避免前端反解析 prompt 文本标记**
/// (`[附加技能 · name]` 曾是前端正则反解的脆弱耦合)。
#[derive(Debug, Clone, Default)]
pub struct RolePreamble {
    pub persona: String,
    pub skills: Vec<SkillPreamble>,
}

/// 单条挂载技能的结构化组成。
#[derive(Debug, Clone)]
pub struct SkillPreamble {
    pub name: String,
    pub instruction: String,
}

impl RolePreamble {
    /// 渲染为最终 system prompt(与原 `compose_role_prompt` 逐字一致)。
    pub fn render(&self) -> String {
        if self.skills.is_empty() {
            return self.persona.clone();
        }
        let skill_text = self
            .skills
            .iter()
            .map(|s| format!("[附加技能 · {}]\n{}", s.name, s.instruction))
            .collect::<Vec<_>>()
            .join("\n\n");
        format!("{}\n\n{skill_text}", self.persona)
    }

    /// 诊断面 JSON(前端直读;shape 稳定)。
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "persona": self.persona,
            "skills": self.skills.iter().map(|s| serde_json::json!({
                "name": s.name, "instruction": s.instruction,
            })).collect::<Vec<_>>(),
        })
    }
}

/// 组装角色 prompt 的结构化组成。`role_id = None` 时用 roles.json 的 active 角色。
/// 返回 None = 无可用提示词(角色缺文件/提示词为空)。
pub fn compose_preamble(data_dir: &Path, role_id: Option<&str>) -> Option<RolePreamble> {
    // 只读消费面:宽容策略(缺/坏 = 无提示词,不阻塞回合)。
    let v = crate::json_store::read_json_lenient(&data_dir.join("config").join("roles.json"))?;
    let (base, mounted): (Option<String>, Vec<String>) = if let Some(roles) = v["roles"].as_array()
    {
        let active = v["active_id"].as_str().unwrap_or("assistant");
        let role = roles
            .iter()
            .find(|r| r["id"].as_str() == Some(role_id.unwrap_or(active)))
            .or_else(|| roles.first());
        (
            role.and_then(|r| r["system_prompt"].as_str().map(String::from)),
            role.and_then(|r| r["skills"].as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        )
    } else {
        // 旧单角色结构(无 skills 概念)
        (v["system_prompt"].as_str().map(String::from), vec![])
    };
    let base = base?;
    // 挂载技能指令追加(skills.json 整体只读/解析一次,不随技能数放大;
    // 缺失或技能缺失则静默跳过)
    let skills_db: Option<serde_json::Value> = crate::json_store::read_json_lenient(
        &crate::ports::skill_host::skills_config_path(data_dir),
    );
    let skills: Vec<SkillPreamble> = mounted
        .iter()
        .filter_map(|sid| {
            skills_db.as_ref()?["skills"]
                .as_array()?
                .iter()
                .find(|s| s["skill_id"].as_str() == Some(sid.as_str()))
                .and_then(|s| {
                    let name = s["name"].as_str().unwrap_or(sid.as_str());
                    let instruction = s["instruction"].as_str()?;
                    Some(SkillPreamble {
                        name: name.to_string(),
                        instruction: instruction.to_string(),
                    })
                })
        })
        .collect();
    Some(RolePreamble {
        persona: base,
        skills,
    })
}

/// 组装角色 system prompt(渲染文本;调用方只需字符串时用之)。
/// `role_id = None` 时用 roles.json 的 active 角色。
/// 返回 None = 无可用提示词(角色缺文件/提示词与技能皆空)。
pub fn compose_role_prompt(data_dir: &Path, role_id: Option<&str>) -> Option<String> {
    compose_preamble(data_dir, role_id).map(|p| p.render())
}

/// 读角色的对话工具白名单(ADR-0022 后续批)。roles.json 角色对象可选
/// `allowed_tools: ["fs.read", "read", "web_search_lite", ...]`(能力名 /
/// 单下划线名 / wire 短名均认,turn 侧归一匹配)。返回:
/// - `None` = 角色未声明白名单(全量挂载,缺省形态);
/// - `Some(list)` = 仅挂清单内工具;空数组语义等同未声明(防手滑全禁)。
pub fn allowed_tools_for(data_dir: &Path, role_id: Option<&str>) -> Option<Vec<String>> {
    let v = crate::json_store::read_json_lenient(&data_dir.join("config").join("roles.json"))?;
    let roles = v["roles"].as_array()?;
    let active = v["active_id"].as_str().unwrap_or("assistant");
    let role = roles
        .iter()
        .find(|r| r["id"].as_str() == Some(role_id.unwrap_or(active)))
        .or_else(|| roles.first())?;
    let list: Vec<String> = role["allowed_tools"]
        .as_array()?
        .iter()
        .filter_map(|t| t.as_str().map(String::from))
        .collect();
    if list.is_empty() { None } else { Some(list) }
}
