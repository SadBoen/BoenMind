//! W4b 角色/技能 prompt 组装:roles.json(角色,含挂载技能列表)+
//! skills.json(技能知识包;合同 capability/skill.v0_1)。
//! 组装规则:激活(或指定)角色的 system_prompt 为基底,其挂载技能的
//! instruction 依次追加。Skill 只是数据,加载不改变权限(基线定义)。
//! 该函数是角色 prompt 的唯一组装点:回合组装(turn.rs)与会话创建
//! (openai_compat)共用,保证两处口径一致。

use std::path::Path;

/// 组装角色 system prompt。`role_id = None` 时用 roles.json 的 active 角色。
/// 返回 None = 无可用提示词(角色缺文件/提示词与技能皆空)。
pub fn compose_role_prompt(data_dir: &Path, role_id: Option<&str>) -> Option<String> {
    let text = std::fs::read_to_string(data_dir.join("config").join("roles.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
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
    // 挂载技能指令追加(skills.json 缺失或技能缺失则静默跳过)
    let skill_text = mounted
        .iter()
        .filter_map(|sid| {
            let raw = std::fs::read_to_string(data_dir.join("config").join("skills.json")).ok()?;
            let sv: serde_json::Value = serde_json::from_str(&raw).ok()?;
            sv["skills"]
                .as_array()?
                .iter()
                .find(|s| s["skill_id"].as_str() == Some(sid.as_str()))
                .and_then(|s| {
                    let name = s["name"].as_str().unwrap_or(sid.as_str());
                    let ins = s["instruction"].as_str()?;
                    Some(format!("[附加技能 · {name}]\n{ins}"))
                })
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if skill_text.is_empty() {
        Some(base)
    } else {
        Some(format!("{base}\n\n{skill_text}"))
    }
}
