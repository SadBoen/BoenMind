//! 工具注册表（A6 骨架定稿接口；B4 pi-compat 的接入点）。
//!
//! B3 从 QuickJS 引擎拿到 `get_registered_tools` 的 ExtensionToolDef 列表后，
//! 按 [`ToolDef`] 形态注册进本表——自研 loop 与 pi 插件工具的唯一汇合点。

use serde::{Deserialize, Serialize};

/// 工具注册错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolError {
    pub message: String,
}

impl ToolError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// 一个已注册工具（模型可见描述）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// 工具参数 JSON Schema（模型可见输入契约）
    pub input_schema: serde_json::Value,
}

impl ToolDef {
    pub fn new(name: impl Into<String>, description: impl Into<String>, input_schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }
}

/// 工具注册表：注册序稳定（模型请求的 tools 数组按注册序）。
#[derive(Debug, Default)]
pub struct ToolRegistry {
    tools: Vec<ToolDef>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// 注册工具；重名拒绝（工具名是 call_id 关联的键，重复会混淆审计链）。
    pub fn register(&mut self, def: ToolDef) -> Result<(), ToolError> {
        if self.tools.iter().any(|t| t.name == def.name) {
            return Err(ToolError::new(format!("tool `{}` already registered", def.name)));
        }
        self.tools.push(def);
        Ok(())
    }

    /// 按名取工具。
    pub fn get(&self, name: &str) -> Option<&ToolDef> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// 全部工具（注册序）。
    pub fn list(&self) -> &[ToolDef] {
        &self.tools
    }

    /// OpenAI 兼容的 tools 数组（模型请求 payload 直接可用）。
    pub fn openai_tools_json(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema,
                        }
                    })
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str) -> ToolDef {
        ToolDef::new(name, "desc", serde_json::json!({"type": "object"}))
    }

    #[test]
    fn register_and_get() {
        let mut r = ToolRegistry::new();
        r.register(def("web_search")).unwrap();
        r.register(def("exec")).unwrap();
        assert!(r.get("web_search").is_some());
        assert!(r.get("nope").is_none());
        assert_eq!(r.list().len(), 2);
    }

    #[test]
    fn duplicate_name_rejected() {
        let mut r = ToolRegistry::new();
        r.register(def("web_search")).unwrap();
        let err = r.register(def("web_search")).unwrap_err();
        assert!(err.message.contains("already registered"));
        assert_eq!(r.list().len(), 1, "重复注册不落表");
    }

    #[test]
    fn openai_tools_array_stable_order() {
        let mut r = ToolRegistry::new();
        r.register(def("b")).unwrap();
        r.register(def("a")).unwrap();
        let arr = r.openai_tools_json();
        let names: Vec<&str> = arr
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["b", "a"], "注册序稳定");
    }
}
