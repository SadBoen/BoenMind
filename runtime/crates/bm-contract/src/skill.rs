//! F3(BACKLOG:skill.v0_1 Rust 强类型投影):SkillDefinition 结构体 +
//! 合同镜像测试。此前 bm-contract 侧仅有 SKILL_SCHEMA JSON 常量,结构
//! 只活在前端 SkillItem 与 roles.rs 的裸 JSON 访问里;本结构体为后续
//! Skill v0.2(wasmtime 执行面)提供类型锚点。只是数据,加载不改变权限。

use serde::{Deserialize, Serialize};

/// skill.v0_1 参考文档条目(references[];相对技能根目录或工作区的路径)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillReference {
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// ADR-0016 第二步:技能脚本条目(scripts[];wasm 执行面)。
/// 注册时合成 CapabilityManifest(capability = skill.<skill_id>.<name>),
/// 经 Broker 七步管线裁决执行,与内置/MCP 能力完全平权(零特权降级)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillScript {
    /// capability 尾段名(`^[a-z][a-z0-9_]{0,31}$`)。
    pub name: String,
    /// wasm 模块路径(相对技能根目录)。
    pub path: String,
    /// 面向模型的一句功能描述(manifest.description)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 风险等级(high-risk 不对技能开放)。
    pub effect: String,
    /// 入参 JSON Schema(Broker 步 4 校验)。
    pub input_schema: serde_json::Value,
    /// 出参 JSON Schema(Broker 步 7 校验)。
    pub output_schema: serde_json::Value,
    /// 执行超时毫秒(默认 10000;wasmtime epoch/fuel 双限)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// skill.v0_1 技能知识包(合同 capability/skill.v0_1.schema.json 镜像;
/// 只是数据:挂载后 instruction 追加进 system prompt,不改变权限)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillDefinition {
    /// 技能稳定标识(挂载引用键;`^skill_[a-z][a-z0-9_]{0,31}$`)。
    #[serde(rename = "skill_id")]
    pub skill_id: String,
    /// 展示名。
    pub name: String,
    /// 技能版本号(0.2 起必填形态;缺省兼容旧包,serde default 兜底)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 一句话描述(列表展示面)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 步骤/提示模板:挂载后追加进 system prompt 的技能指令正文。
    pub instruction: String,
    /// 允许的能力清单(Skill v0.2 执行面引用;阶段一只透传)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_capabilities: Option<Vec<String>>,
    /// 参考文档(references[];Skill v0.2 第一步增发)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<SkillReference>>,
    /// 技能脚本(scripts[];ADR-0016 第二步增发;缺省 = 纯知识包)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<Vec<SkillScript>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合同镜像:合法 skill 载荷双向往返,字段不丢。
    #[test]
    fn skill_definition_roundtrip() {
        let raw = serde_json::json!({
            "skill_id": "skill_rhyming_poet",
            "name": "押韵诗人",
            "version": "0.2",
            "description": "任何提问都用打油诗回答",
            "instruction": "无论用户问什么,你都必须用中文打油诗回答。",
            "allowed_capabilities": ["fs.read"],
            "references": [
                {"name": "韵脚表", "path": "docs/rhymes.md", "description": "常用韵脚"}
            ]
        });
        let def: SkillDefinition = serde_json::from_value(raw.clone()).expect("合法 skill");
        assert_eq!(def.skill_id, "skill_rhyming_poet");
        assert_eq!(def.version.as_deref(), Some("0.2"));
        assert_eq!(def.references.as_ref().map(|r| r.len()), Some(1));
        let back = serde_json::to_value(&def).expect("序列化");
        assert_eq!(back, raw, "往返零漂移");
    }

    /// 最小合法形态:required 仅 skill_id/name/instruction;可选字段缺省不报。
    #[test]
    fn skill_definition_minimal_shape() {
        let def: SkillDefinition = serde_json::from_value(serde_json::json!({
            "skill_id": "skill_min",
            "name": "最小",
            "instruction": "只做一件事。"
        }))
        .expect("最小形态合法");
        assert_eq!(def.version, None);
        assert_eq!(def.references, None);
        // 落盘形态:可选字段不出字段(与前端 SkillItem 可选口径一致)
        let v = serde_json::to_value(&def).expect("序列化");
        assert!(v.get("version").is_none());
        assert!(v.get("references").is_none());
    }
    /// ADR-0016 第二步:scripts 字段合同镜像(wasm 执行面)。
    #[test]
    fn skill_scripts_roundtrip() {
        let raw = serde_json::json!({
            "skill_id": "skill_units",
            "name": "单位换算",
            "instruction": "需要换算时调用脚本。",
            "scripts": [
                {
                    "name": "convert",
                    "path": "scripts/convert.wasm",
                    "description": "长度/重量单位换算",
                    "effect": "read-only",
                    "input_schema": {"type": "object"},
                    "output_schema": {"type": "object"},
                    "timeout_ms": 5000
                }
            ]
        });
        let def: SkillDefinition = serde_json::from_value(raw.clone()).expect("合法");
        let scripts = def.scripts.as_ref().expect("scripts 在场");
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].name, "convert");
        assert_eq!(scripts[0].effect, "read-only");
        assert_eq!(scripts[0].timeout_ms, Some(5000));
        assert_eq!(
            serde_json::to_value(&def).expect("序列化"),
            raw,
            "往返零漂移"
        );
    }

    /// 纯知识包兼容:无 scripts 字段 = None(落盘不出字段)。
    #[test]
    fn skill_without_scripts_is_pure_knowledge_pack() {
        let def: SkillDefinition = serde_json::from_value(serde_json::json!({
            "skill_id": "skill_plain",
            "name": "纯知识",
            "instruction": "只给知识。"
        }))
        .expect("合法");
        assert_eq!(def.scripts, None);
        let v = serde_json::to_value(&def).expect("序列化");
        assert!(v.get("scripts").is_none(), "缺省不出字段");
    }
}
