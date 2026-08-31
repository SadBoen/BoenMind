//! 用户可改配置的存储与 CRUD(D-M3-1 配置管理批次,ADR-0012)。
//!
//! 配置节 = 命名空间(ns)+ 数据目录下一份人可读的 JSON 文件
//! (`config/<ns>.json`)。逐字段读取优先级:**配置文件 > 启动 env > 内置默认**。
//! 服务器启动(boenmind-server)与本模块共用同一份合并逻辑,保证
//! 「界面保存 → 重启生效」前后读到同一个值;生效时机 = 下次启动(v0)。
//!
//! secret 字段(apiKey)明文只落配置文件(与既有 .secrets/dev.env 明文同级,
//! 不倒退);运行时凭据仍由服务器播种进加密 Secret Store(INV-5 面不变)。
//! API 回显恒打码:values 中 secret 字段为 null,是否已设置由 secret_set 标记。
//!
//! v0 只有 `model` 一个配置节;机制通用,新增配置节 = 注册 schema + 字段校验。

use bm_contract::wire::{ConfigFieldInfo, ConfigListResult, ConfigSectionInfo, ConfigValuesResult};
use bm_core::error::CoreResult;
use bm_core::CoreError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// v0 唯一配置节:模型接入(OpenAI 兼容网关)。
pub const MODEL_NS: &str = "model";

/// 配置文件相对数据目录的路径。
pub fn section_rel_path(ns: &str) -> Option<&'static str> {
    match ns {
        MODEL_NS => Some("config/model.json"),
        _ => None,
    }
}

fn section_file(data_dir: &Path, ns: &str) -> Option<PathBuf> {
    section_rel_path(ns).map(|rel| data_dir.join(rel))
}

fn validation(msg: impl Into<String>) -> CoreError {
    CoreError::validation(msg)
}

/// 已知字段名(delete 按名删;不校验值)。
fn known_field(name: &str) -> bool {
    matches!(name, "baseUrl" | "apiKey" | "modelId" | "stream" | "displayName" | "models")
}

/// 字段校验与类型约束(v0:model 节五字段;未知字段拒绝)。
fn validate_field(name: &str, value: &Value) -> CoreResult<()> {
    match name {
        "baseUrl" => {
            let s = value
                .as_str()
                .ok_or_else(|| validation("baseUrl 必须是字符串"))?;
            if !(s.len() <= 500 && (s.starts_with("http://") || s.starts_with("https://"))) {
                return Err(validation("baseUrl 必须以 http:// 或 https:// 开头(≤500 字符)"));
            }
        }
        "apiKey" => {
            let s = value
                .as_str()
                .ok_or_else(|| validation("apiKey 必须是字符串"))?;
            if s.is_empty() || s.len() > 4096 {
                return Err(validation("apiKey 不能为空且 ≤4096 字符"));
            }
        }
        "modelId" => {
            let s = value
                .as_str()
                .ok_or_else(|| validation("modelId 必须是字符串"))?;
            if s.is_empty() || s.len() > 200 {
                return Err(validation("modelId 不能为空且 ≤200 字符"));
            }
        }
        "stream" => {
            if !value.is_boolean() {
                return Err(validation("stream 必须是布尔值"));
            }
        }
        "displayName" => {
            let s = value
                .as_str()
                .ok_or_else(|| validation("displayName 必须是字符串"))?;
            if s.is_empty() || s.len() > 100 {
                return Err(validation("displayName 不能为空且 ≤100 字符"));
            }
        }
        "models" => {
            let arr = value
                .as_array()
                .ok_or_else(|| validation("models 必须是字符串数组"))?;
            if arr.len() > 50 {
                return Err(validation("models 至多 50 个模型"));
            }
            for m in arr {
                let id = m.as_str().ok_or_else(|| validation("models 项必须是字符串"))?;
                if id.is_empty() || id.len() > 200 {
                    return Err(validation("models 项不能为空且 ≤200 字符"));
                }
            }
        }
        other => return Err(validation(format!("未知配置字段 '{other}'"))),
    }
    Ok(())
}

/// 读配置节文件;缺失/损坏 → 空对象(损坏文件不阻塞服务,下次 set 覆盖)。
fn read_file(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_file(path: &Path, value: &Value) -> CoreResult<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| validation(format!("配置目录创建失败: {e}")))?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|_| CoreError::Internal)?
        .replace("\n", "\r\n");
    std::fs::write(path, text).map_err(|e| validation(format!("配置文件写入失败: {e}")))
}

/// env 读取的纯函数内核(便于测试:env 值由调用方传入)。
/// 注:apiKey 在投影中恒为 null(打码),env 密钥只参与 secret_set 标记。
fn effective_from(
    file: &Value,
    env_base_url: Option<&str>,
    env_model_id: Option<&str>,
    env_stream: bool,
) -> Value {
    let pick_str = |file_key: &str, env: Option<&str>| -> Value {
        file[file_key]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| json!(s))
            .or_else(|| env.filter(|s| !s.is_empty()).map(|s| json!(s)))
            .unwrap_or(Value::Null)
    };
    json!({
        "baseUrl": pick_str("baseUrl", env_base_url),
        "apiKey": Value::Null,
        "modelId": pick_str("modelId", env_model_id),
        "stream": file["stream"].as_bool().unwrap_or(env_stream),
        "displayName": pick_str("displayName", None),
    })
}

fn secret_set_from(file: &Value, env_api_key: Option<&str>) -> Value {
    let file_set = file["apiKey"].as_str().map(|s| !s.is_empty()).unwrap_or(false);
    let env_set = env_api_key.map(|s| !s.is_empty()).unwrap_or(false);
    json!({ "apiKey": file_set || env_set })
}

/// 用户可改配置存储。无锁:每次操作独立读写文件,个人单机形态无并发竞争;
/// 文件为权威,进程内不缓存(重启生效语义天然一致)。
pub struct ConfigStore {
    data_dir: PathBuf,
}

impl ConfigStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self { data_dir: data_dir.into() }
    }

    /// config.list
    pub fn list(&self) -> ConfigListResult {
        ConfigListResult {
            sections: vec![ConfigSectionInfo {
                ns: MODEL_NS.to_string(),
                title: "模型接入(OpenAI 兼容网关)".to_string(),
                description: Some(
                    "API 地址 / 密钥 / 模型名;保存后重启服务生效。逐字段优先级:配置文件 > 启动环境变量 > 默认。"
                        .to_string(),
                ),
                file: section_rel_path(MODEL_NS).map(|s| s.to_string()),
                fields: vec![
                    ConfigFieldInfo {
                        name: "baseUrl".to_string(),
                        kind: "string".to_string(),
                        secret: false,
                        description: Some("API 地址(OpenAI 兼容,含 /v1 等前缀)".to_string()),
                    },
                    ConfigFieldInfo {
                        name: "apiKey".to_string(),
                        kind: "string".to_string(),
                        secret: true,
                        description: Some("API 密钥(读取恒打码;留空保存 = 保持不变)".to_string()),
                    },
                    ConfigFieldInfo {
                        name: "modelId".to_string(),
                        kind: "string".to_string(),
                        secret: false,
                        description: Some("模型 ID".to_string()),
                    },
                    ConfigFieldInfo {
                        name: "stream".to_string(),
                        kind: "boolean".to_string(),
                        secret: false,
                        description: Some("流式输出(缺省回落 BOEN_MODEL_STREAM 环境变量)".to_string()),
                    },
                    ConfigFieldInfo {
                        name: "displayName".to_string(),
                        kind: "string".to_string(),
                        secret: false,
                        description: Some("显示名称(可选,界面分组名)".to_string()),
                    },
                    ConfigFieldInfo {
                        name: "models".to_string(),
                        kind: "string".to_string(),
                        secret: false,
                        description: Some("模型清单(界面「获取可用模型」勾选保存;空 = 只用 modelId)".to_string()),
                    },
                ],
            }],
        }
    }

    /// config.get:生效值投影(secret 打码)。
    pub fn get(&self, ns: &str) -> CoreResult<ConfigValuesResult> {
        let path = section_file(&self.data_dir, ns)
            .ok_or_else(|| validation(format!("未知配置节 '{ns}'")))?;
        let file = read_file(&path);
        Ok(ConfigValuesResult {
            ns: ns.to_string(),
            values: effective_from(
                &file,
                non_empty_env("BOEN_MODEL_BASE_URL").as_deref(),
                non_empty_env("BOEN_MODEL_ID").as_deref(),
                std::env::var("BOEN_MODEL_STREAM").as_deref() == Ok("1"),
            ),
            secret_set: secret_set_from(&file, non_empty_env("BOEN_MODEL_API_KEY").as_deref()),
        })
    }

    /// config.set:增量合并写入;secret 留空(null/空串/缺省)= 保持不变。
    pub fn set(&self, ns: &str, values: &Value) -> CoreResult<ConfigValuesResult> {
        let map = values
            .as_object()
            .ok_or_else(|| validation("values 必须是对象"))?;
        let path = section_file(&self.data_dir, ns)
            .ok_or_else(|| validation(format!("未知配置节 '{ns}'")))?;
        let mut file = read_file(&path);
        let obj = file.as_object_mut().ok_or(CoreError::Internal)?;
        for (key, value) in map {
            // secret 留空 = 不改(ADR-0012 密钥口径)
            if key == "apiKey" && (value.is_null() || value.as_str() == Some("")) {
                continue;
            }
            validate_field(key, value)?;
            obj.insert(key.clone(), value.clone());
        }
        write_file(&path, &file)?;
        self.get(ns)
    }

    /// config.delete:指定字段 = 删该字段;缺省 = 整节复位(删文件)。
    pub fn delete(&self, ns: &str, field: Option<&str>) -> CoreResult<ConfigValuesResult> {
        let path = section_file(&self.data_dir, ns)
            .ok_or_else(|| validation(format!("未知配置节 '{ns}'")))?;
        match field {
            Some(field) => {
                // 字段名必须已知(防误删任意键)
                if !known_field(field) {
                    return Err(validation(format!("未知配置字段 '{field}'")));
                }
                let mut file = read_file(&path);
                if let Some(obj) = file.as_object_mut() {
                    obj.remove(field);
                }
                write_file(&path, &file)?;
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        self.get(ns)
    }
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// 服务器启动用的模型接入生效配置(逐字段:文件 > env > 默认)。
/// boenmind-server 据此装配 connector / 播种密钥 / 流式开关,与本模块
/// config.get 回显同源(ADR-0012)。
#[derive(Debug, Clone, Default)]
pub struct EffectiveModel {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model_id: Option<String>,
    pub stream: bool,
    pub display_name: Option<String>,
    /// 模型清单(界面「获取可用模型」后勾选保存的列表;空 = 只用 model_id)。
    pub models: Vec<String>,
}

pub fn effective_model(data_dir: &Path) -> EffectiveModel {
    let file = section_file(data_dir, MODEL_NS).map(|p| read_file(&p)).unwrap_or(json!({}));
    let eff = effective_from(
        &file,
        non_empty_env("BOEN_MODEL_BASE_URL").as_deref(),
        non_empty_env("BOEN_MODEL_ID").as_deref(),
        std::env::var("BOEN_MODEL_STREAM").as_deref() == Ok("1"),
    );
    let s = |v: &Value| v.as_str().map(|x| x.to_string());
    EffectiveModel {
        base_url: s(&eff["baseUrl"]).filter(|x| !x.is_empty()),
        api_key: file["apiKey"]
            .as_str()
            .map(|x| x.to_string())
            .filter(|x| !x.is_empty())
            .or_else(|| non_empty_env("BOEN_MODEL_API_KEY")),
        model_id: s(&eff["modelId"]).filter(|x| !x.is_empty()),
        stream: eff["stream"].as_bool().unwrap_or(false),
        display_name: s(&eff["displayName"]).filter(|x| !x.is_empty()),
        models: file["models"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m.as_str().map(|x| x.to_string()))
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tmp: &Path) -> ConfigStore {
        ConfigStore::new(tmp)
    }

    #[test]
    fn t_cfg_set_get_delete_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        assert_eq!(s.list().sections.len(), 1);
        assert_eq!(s.list().sections[0].ns, "model");

        let r = s
            .set("model", &json!({"baseUrl": "https://api.example.com/v1", "modelId": "glm-5.3"}))
            .unwrap();
        assert_eq!(r.values["baseUrl"], json!("https://api.example.com/v1"));
        assert_eq!(r.values["modelId"], json!("glm-5.3"));
        // 文件确已落盘且人可读
        let raw = std::fs::read_to_string(tmp.path().join("config/model.json")).unwrap();
        assert!(raw.contains("baseUrl"));

        // 删字段回落;整节复位后 baseUrl 投影为 null(无 env)
        s.delete("model", Some("modelId")).unwrap();
        assert_eq!(s.get("model").unwrap().values["modelId"], Value::Null);
        s.delete("model", None).unwrap();
        assert_eq!(s.get("model").unwrap().values["baseUrl"], Value::Null);
    }

    #[test]
    fn t_cfg_secret_masking_and_keep_on_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        let r = s.set("model", &json!({"apiKey": "sk-secret-1"})).unwrap();
        assert_eq!(r.values["apiKey"], Value::Null, "回显必须打码");
        assert_eq!(r.secret_set["apiKey"], json!(true));
        // 留空保存 = 保持不变
        let r = s.set("model", &json!({"apiKey": null, "modelId": "m1"})).unwrap();
        assert_eq!(r.secret_set["apiKey"], json!(true));
        // 显示层面仍无明文
        assert_eq!(r.values["apiKey"], Value::Null);
        // 显式清除走 delete(field)
        let r = s.delete("model", Some("apiKey")).unwrap();
        assert_eq!(r.secret_set["apiKey"], json!(false));
    }

    #[test]
    fn t_cfg_validation_rejects_unknown_and_bad_values() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store(tmp.path());
        assert!(s.set("model", &json!({"nope": 1})).is_err());
        assert!(s.set("model", &json!({"baseUrl": "ftp://x"})).is_err());
        assert!(s.set("model", &json!({"modelId": ""})).is_err());
        assert!(s.set("model", &json!({"stream": "yes"})).is_err());
        assert!(s.get("nope").is_err());
    }

    #[test]
    fn t_cfg_effective_merge_file_over_env() {
        let file = json!({"baseUrl": "https://file.example.com/v1", "stream": true});
        let eff = effective_from(&file, Some("https://env.example.com"), Some("env-model"), false);
        assert_eq!(eff["baseUrl"], json!("https://file.example.com/v1"), "文件优先");
        assert_eq!(eff["stream"], json!(true));
        assert_eq!(eff["modelId"], json!("env-model"), "文件缺省回落 env");
        // 全缺省
        let eff = effective_from(&json!({}), None, None, false);
        assert_eq!(eff["baseUrl"], Value::Null);
        assert_eq!(eff["stream"], json!(false));
    }
}
