//! 当前生效模型的配置存储(W2 从 archive/m10-dsh-frontend 恢复接线;
//! 机制沿 ADR-0012:数据目录下一份人可读 JSON `config/model.json`,
//! 逐字段优先级 **配置文件 > 启动 env > 内置默认**,变更落盘后下次启动生效)。
//!
//! 与归档版的差异:只保留「当前生效模型」单节机制(effective_model /
//! set_active / 打码投影),不再承载 dsh config.list/get/set 协议方法
//! (该方法族随 dsh 线归档,ADR-0013;W2 管理面 = webadmin.rs 的
//! REST 端点,壳子私用,见 webadmin.rs 模块注释)。多 provider 实体库
//! 在 webadmin.rs 的 providers.json,与本节的关系:管理面「设为当前」
//! 把选中 provider 字段落入本节,服务器下次启动按本节装配 connector。
//!
//! secret 字段(apiKey)明文只落配置文件(与既有 .secrets/dev.env 明文同级,
//! 不倒退);运行时凭据仍由服务器播种进加密 Secret Store(INV-5 面不变)。
//! API 回显恒打码:apiKey 为 null,是否已设置由 secret_set 标记承载。

use bm_core::CoreError;
use bm_core::CoreResult;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// 配置文件相对数据目录的路径。
pub const MODEL_REL_PATH: &str = "config/model.json";

fn section_file(data_dir: &Path) -> PathBuf {
    data_dir.join(MODEL_REL_PATH)
}

fn validation(msg: impl Into<String>) -> CoreError {
    CoreError::validation(msg)
}

/// 已知字段名(delete 按名删;不校验值)。
fn known_field(name: &str) -> bool {
    matches!(
        name,
        "baseUrl" | "apiKey" | "modelId" | "stream" | "displayName" | "models" | "contextWindows"
    )
}

/// 字段校验与类型约束(未知字段拒绝)。
pub fn validate_field(name: &str, value: &Value) -> CoreResult<()> {
    match name {
        "baseUrl" => {
            let s = value
                .as_str()
                .ok_or_else(|| validation("baseUrl 必须是字符串"))?;
            if !(s.len() <= 500 && (s.starts_with("http://") || s.starts_with("https://"))) {
                return Err(validation(
                    "baseUrl 必须以 http:// 或 https:// 开头(≤500 字符)",
                ));
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
                let id = m
                    .as_str()
                    .ok_or_else(|| validation("models 项必须是字符串"))?;
                if id.is_empty() || id.len() > 200 {
                    return Err(validation("models 项不能为空且 ≤200 字符"));
                }
            }
        }
        "contextWindows" => {
            // 模型窗口登记表(model_id → 上下文窗口 token 数);透视面板
            // 「真实水位」唯一数据源——未登记即如实显示「窗口未知」。
            let obj = value
                .as_object()
                .ok_or_else(|| validation("contextWindows 必须是对象(模型 → 窗口 token 数)"))?;
            if obj.len() > 50 {
                return Err(validation("contextWindows 至多 50 条登记"));
            }
            for (k, v) in obj {
                if k.is_empty() || k.len() > 200 {
                    return Err(validation("contextWindows 的模型名不能为空且 ≤200 字符"));
                }
                let n = v
                    .as_u64()
                    .ok_or_else(|| validation("contextWindows 值必须是正整数(token 数)"))?;
                if !(1..=4_000_000).contains(&n) {
                    return Err(validation(
                        "contextWindows 值超出合理区间(1..=4,000,000 token)",
                    ));
                }
            }
        }
        other => return Err(validation(format!("未知配置字段 '{other}'"))),
    }
    Ok(())
}

/// 读配置文件;缺失/损坏 → 空对象(损坏文件不阻塞服务,下次 set 覆盖)。
fn read_file(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

/// pretty JSON → CRLF 文本(Windows 人可读口径;webadmin 配置写入共用)。
pub fn crlf(pretty: String) -> String {
    pretty.replace('\n', "\r\n")
}

fn write_file(path: &Path, value: &Value) -> CoreResult<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| validation(format!("配置目录创建失败: {e}")))?;
    }
    let text = crlf(serde_json::to_string_pretty(value).map_err(|_| CoreError::Internal)?);
    bm_persist::atomic_write(path, text.as_bytes())
        .map_err(|e| validation(format!("配置文件写入失败: {e}")))
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
        "contextWindows": file.get("contextWindows").cloned().unwrap_or(Value::Null),
    })
}

fn secret_set_from(file: &Value, env_api_key: Option<&str>) -> Value {
    let file_set = file["apiKey"]
        .as_str()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let env_set = env_api_key.map(|s| !s.is_empty()).unwrap_or(false);
    json!({ "apiKey": file_set || env_set })
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// 用户可改配置存储。无锁:每次操作独立读写文件,个人单机形态无并发竞争;
/// 文件为权威,进程内不缓存(重启生效语义天然一致)。
pub struct ModelConfigStore {
    data_dir: PathBuf,
}

impl ModelConfigStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// 生效值投影(secret 打码)。
    pub fn get(&self) -> Value {
        let file = read_file(&section_file(&self.data_dir));
        json!({
            "values": effective_from(
                &file,
                non_empty_env("BOEN_MODEL_BASE_URL").as_deref(),
                non_empty_env("BOEN_MODEL_ID").as_deref(),
                std::env::var("BOEN_MODEL_STREAM").as_deref() == Ok("1"),
            ),
            "secret_set": secret_set_from(&file, non_empty_env("BOEN_MODEL_API_KEY").as_deref()),
        })
    }

    /// 增量合并写入;secret 留空(null/空串/缺省)= 保持不变(ADR-0012 口径)。
    pub fn set(&self, values: &Value) -> CoreResult<Value> {
        let map = values
            .as_object()
            .ok_or_else(|| validation("values 必须是对象"))?;
        let path = section_file(&self.data_dir);
        let mut file = read_file(&path);
        let obj = file.as_object_mut().ok_or(CoreError::Internal)?;
        for (key, value) in map {
            if key == "apiKey" && (value.is_null() || value.as_str() == Some("")) {
                continue;
            }
            validate_field(key, value)?;
            obj.insert(key.clone(), value.clone());
        }
        write_file(&path, &file)?;
        Ok(self.get())
    }

    /// 删字段(回落 env/默认);字段名必须已知(防误删任意键)。
    pub fn delete_field(&self, field: &str) -> CoreResult<Value> {
        if !known_field(field) {
            return Err(validation(format!("未知配置字段 '{field}'")));
        }
        let path = section_file(&self.data_dir);
        let mut file = read_file(&path);
        if let Some(obj) = file.as_object_mut() {
            obj.remove(field);
        }
        write_file(&path, &file)?;
        Ok(self.get())
    }
}

/// 服务器启动用的模型接入生效配置(逐字段:文件 > env > 默认)。
/// boenmind-server 据此装配 connector / 播种密钥 / 流式开关,与
/// ModelConfigStore::get 回显同源(ADR-0012)。
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
    let file = read_file(&section_file(data_dir));
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

    #[test]
    fn t_cfg_set_get_delete_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ModelConfigStore::new(tmp.path());
        let r = s
            .set(&json!({"baseUrl": "https://api.example.com/v1", "modelId": "glm-5.3"}))
            .unwrap();
        assert_eq!(r["values"]["baseUrl"], json!("https://api.example.com/v1"));
        assert_eq!(r["values"]["modelId"], json!("glm-5.3"));
        // 文件确已落盘且人可读
        let raw = std::fs::read_to_string(tmp.path().join("config/model.json")).unwrap();
        assert!(raw.contains("baseUrl"));

        // 删字段回落;投影为 null(无 env 时)
        s.delete_field("modelId").unwrap();
        assert_eq!(s.get()["values"]["modelId"], Value::Null);
    }

    #[test]
    fn t_cfg_secret_masking_and_keep_on_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ModelConfigStore::new(tmp.path());
        let r = s.set(&json!({"apiKey": "sk-secret-1"})).unwrap();
        assert_eq!(r["values"]["apiKey"], Value::Null, "回显必须打码");
        assert_eq!(r["secret_set"]["apiKey"], json!(true));
        // 留空保存 = 保持不变
        let r = s.set(&json!({"apiKey": null, "modelId": "m1"})).unwrap();
        assert_eq!(r["secret_set"]["apiKey"], json!(true));
        assert_eq!(r["values"]["apiKey"], Value::Null);
        // 显式清除走 delete_field
        let r = s.delete_field("apiKey").unwrap();
        assert_eq!(r["secret_set"]["apiKey"], json!(false));
    }

    #[test]
    fn t_cfg_validation_rejects_unknown_and_bad_values() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ModelConfigStore::new(tmp.path());
        assert!(s.set(&json!({"nope": 1})).is_err());
        assert!(s.set(&json!({"baseUrl": "ftp://x"})).is_err());
        assert!(s.set(&json!({"modelId": ""})).is_err());
        assert!(s.set(&json!({"stream": "yes"})).is_err());
        assert!(s.delete_field("arbitrary").is_err());
    }

    #[test]
    fn t_cfg_context_windows_roundtrip_and_validation() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ModelConfigStore::new(tmp.path());
        let r = s
            .set(&json!({"contextWindows": {"mimo-v2.5": 128000, "glm-5.3": 200000}}))
            .unwrap();
        assert_eq!(r["values"]["contextWindows"]["mimo-v2.5"], json!(128000));
        // 增量写不丢(另一字段更新后登记仍在)
        s.set(&json!({"modelId": "mimo-v2.5"})).unwrap();
        assert_eq!(
            s.get()["values"]["contextWindows"]["glm-5.3"],
            json!(200000)
        );
        // 非法形态一律拒收
        assert!(s.set(&json!({"contextWindows": "big"})).is_err());
        assert!(s.set(&json!({"contextWindows": {"m": 0}})).is_err());
        assert!(s.set(&json!({"contextWindows": {"m": -1}})).is_err());
        assert!(s.set(&json!({"contextWindows": {"": 128000}})).is_err());
        assert!(s.set(&json!({"contextWindows": {"m": 5_000_000}})).is_err());
    }

    #[test]
    fn t_cfg_effective_merge_file_over_env() {
        let file = json!({"baseUrl": "https://file.example.com/v1", "stream": true});
        let eff = effective_from(
            &file,
            Some("https://env.example.com"),
            Some("env-model"),
            false,
        );
        assert_eq!(
            eff["baseUrl"],
            json!("https://file.example.com/v1"),
            "文件优先"
        );
        assert_eq!(eff["stream"], json!(true));
        assert_eq!(eff["modelId"], json!("env-model"), "文件缺省回落 env");
        let eff = effective_from(&json!({}), None, None, false);
        assert_eq!(eff["baseUrl"], Value::Null);
        assert_eq!(eff["stream"], json!(false));
    }

    #[test]
    fn t_cfg_effective_model_reads_file_key() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ModelConfigStore::new(tmp.path());
        s.set(&json!({
            "baseUrl": "https://opencode.ai/zen/go/v1",
            "apiKey": "sk-file-key",
            "modelId": "mimo-v2.5",
            "models": ["mimo-v2.5", "gpt-5.6"]
        }))
        .unwrap();
        let eff = effective_model(tmp.path());
        assert_eq!(
            eff.base_url.as_deref(),
            Some("https://opencode.ai/zen/go/v1")
        );
        assert_eq!(
            eff.api_key.as_deref(),
            Some("sk-file-key"),
            "文件密钥优先于 env"
        );
        assert_eq!(eff.model_id.as_deref(), Some("mimo-v2.5"));
        assert_eq!(eff.models.len(), 2);
    }
}
