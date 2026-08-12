//! 插件设置：manifest `settings` schema 解析 + 持久化 + 密钥掩码。
//!
//! 设置页注册机制的领域层：
//! - 插件在 `extension.json` 里声明 `settings` 数组（字段类型/默认值/选项），
//!   前端按此 schema 动态渲染表单；
//! - 值存于**插件目录内** `~/.boenmind/extensions/<id>/settings.json`（扁平
//!   `{key: value}`）：QuickJS 沙箱的 node:fs 读被限制在 workspace 与扩展根
//!   目录内，用户级目录（~/.boenmind/plugin-settings）沙箱读不到；放扩展根
//!   内插件可直接读取，且卸载插件时配置随目录一并删除；
//! - `secret` 字段读取时打掩码（`sk-12****`），密钥明文只在文件里，不回传
//!   前端。保存语义：提交空字符串或与掩码完全相等 = "未修改"保留原值；
//!   显式清除用 `__clear.<key>: true` 标记（旧版"提交空 = 清除"已废弃，
//!   空框误提交会静默清掉密钥，不安全）。
//!
//! 同文件还承载另两个 manifest 声明（同为"设置页注册机制"的一部分）：
//! - `quota`：用量文件路径（相对工作文件夹）与按次计费源清单，服务端据此
//!   读取/累加用量（见 bm-server routes），插件自身也写同一文件；
//! - `testSources`：设置页「测试」按钮的探测请求模板，服务端按模板发轻量
//!   请求验证连通（模板 `{<settings key>}` 用当前设置值替换），新增搜索源
//!   无需改服务端代码。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use crate::plugins::plugins_dir;

/// manifest settings 数组里一个字段的声明。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingField {
    /// 点分路径式 key（如 `sources.jina.apiKey`），前端按 `.` 分组展示；
    /// Group 类型用 `*` 通配（如 `custom*`），按实例展开为 custom1/custom2…
    pub key: String,
    #[serde(rename = "type")]
    pub field_type: SettingFieldType,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: String,
    /// select 类型的候选值
    #[serde(default)]
    pub options: Vec<String>,
    /// number 类型的取值范围
    #[serde(default)]
    pub min: Option<i64>,
    #[serde(default)]
    pub max: Option<i64>,
    #[serde(default)]
    pub default: Option<Value>,
    /// Group 类型的子字段模板（key 为相对 key，如 `enabled`/`name`）
    #[serde(default)]
    pub fields: Vec<SettingField>,
    /// Group 类型的默认实例数（如 2 → custom1/custom2；前端可追加更多）
    #[serde(default = "default_group_instances")]
    pub instances: usize,
    /// 组的显示名（放在组内第一个字段上声明，如 `search.mode` 声明
    /// "搜索设置"），前端按点分路径分组展示时用作组标题
    #[serde(default)]
    pub group_label: String,
}

fn default_group_instances() -> usize {
    2
}

/// manifest `quota` 声明：用量文件路径 + 按次计费源清单。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaDecl {
    /// 用量文件路径（相对工作文件夹，如 `.boenmind/web-search/quota.json`）
    pub path: String,
    /// 测试成功即消耗免费额度的按次计费源（测试按钮会真实扣一次，服务端累加）
    #[serde(default)]
    pub count_on_test: Vec<String>,
}

/// manifest `testSources` 里单个源的探测请求模板。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSourceDecl {
    /// GET / POST
    pub method: String,
    /// 请求 URL；`{<settings key>}` 会被替换为当前设置值
    pub url: String,
    /// 请求头（顺序无关）；值为模板，替换结果为空 = 该依赖未配置
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    /// JSON body（POST 时发送）；值为模板，可整体替换
    #[serde(default)]
    pub body: Option<Value>,
    /// 设置页测试按钮展示名（缺省用源 id）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// 从 manifest JSON 解析 `quota` 声明（无该段返回 None）。
pub fn parse_quota_decl(manifest: &Value) -> Option<QuotaDecl> {
    let decl = manifest.get("quota")?;
    serde_json::from_value::<QuotaDecl>(decl.clone())
        .ok()
        .filter(|q| !q.path.is_empty())
}

/// 从 manifest JSON 解析 `testSources` 声明：源 id（含 `custom*` 通配）→ 模板。
pub fn parse_test_sources(manifest: &Value) -> Option<HashMap<String, TestSourceDecl>> {
    let map = manifest.get("testSources")?.as_object()?;
    let decls: HashMap<String, TestSourceDecl> = map
        .iter()
        .filter_map(|(id, v)| {
            let mut decl: TestSourceDecl = serde_json::from_value(v.clone()).ok()?;
            if decl.method.is_empty() || decl.url.is_empty() {
                return None;
            }
            decl.method = decl.method.to_ascii_uppercase();
            if decl.label.is_none() {
                decl.label = Some(id.clone());
            }
            Some((id.clone(), decl))
        })
        .collect();
    (!decls.is_empty()).then_some(decls)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingFieldType {
    String,
    /// 密钥：读取时掩码，保存时掩码值保留原值
    Secret,
    Boolean,
    Number,
    Select,
    /// 通配分组：按实例数展开为多个重复字段组（前端提供"添加"交互）
    Group,
}

/// 展开后的叶子字段：完整扁平 key + 字段模板（Group 已按实例展开）。
struct FlatField {
    key: String,
    field: SettingField,
}

/// 把 schema 展开为扁平叶子列表：Group 字段按实例数（默认 + 已存文件中的实际数）
/// 展开为 `custom<N>.<subkey>`。
fn expand_schema(schema: &[SettingField], saved: &serde_json::Map<String, Value>) -> Vec<FlatField> {
    let mut out = Vec::new();
    for f in schema {
        if f.field_type == SettingFieldType::Group {
            // 实际实例数 = max(默认, 文件里已存在的 customN.*)
            let mut actual = f.instances;
            for key in saved.keys() {
                if let Some(rest) = key.strip_prefix(&f.key.replace('*', ""))
                    && let Some(num) = rest.split('.').next().and_then(|n| n.parse::<usize>().ok())
                {
                    actual = actual.max(num);
                }
            }
            let prefix = f.key.replace('*', "");
            for n in 1..=actual {
                for sub in &f.fields {
                    out.push(FlatField {
                        key: format!("{prefix}{n}.{}", sub.key),
                        field: sub.clone(),
                    });
                }
            }
        } else {
            out.push(FlatField { key: f.key.clone(), field: f.clone() });
        }
    }
    out
}

/// 从 manifest JSON 解析 settings schema（无 settings 字段返回 None）。
pub fn parse_settings_schema(manifest: &Value) -> Option<Vec<SettingField>> {
    let arr = manifest.get("settings")?.as_array()?;
    let fields: Vec<SettingField> = arr
        .iter()
        .filter_map(|v| serde_json::from_value::<SettingField>(v.clone()).ok())
        .filter(|f| !f.key.is_empty())
        .collect();
    (!fields.is_empty()).then_some(fields)
}

/// 插件设置文件：跟随插件目录（扩展根内，沙箱可读；卸载插件时一并删除）。
/// 仅 manifest（目录型）插件有 settings schema，因此目录必然存在。
fn settings_file(id: &str) -> PathBuf {
    plugins_dir().join(id).join("settings.json")
}

/// 读取插件设置：文件中的值 + schema 默认值合并（缺字段补默认）。
/// 返回扁平 `{key: value}`；文件缺失/损坏时仅返回默认值。
pub fn read_settings(id: &str, schema: &[SettingField]) -> Value {
    let mut out = serde_json::Map::new();
    let saved = load_saved_map(id);
    // 先铺默认值（Group 已按实例展开）
    for flat in expand_schema(schema, &saved) {
        if let Some(d) = &flat.field.default {
            out.insert(flat.key.clone(), d.clone());
        }
    }
    // 再覆盖已保存的值（仅 schema 声明的 key，防污染）
    for flat in expand_schema(schema, &saved) {
        if let Some(v) = saved.get(&flat.key) {
            out.insert(flat.key.clone(), v.clone());
        }
    }
    Value::Object(out)
}

/// 读取设置文件为 Map（不存在/损坏时返回空）。
fn load_saved_map(id: &str) -> serde_json::Map<String, Value> {
    if let Ok(text) = fs::read_to_string(settings_file(id))
        && let Ok(Value::Object(saved)) = serde_json::from_str::<Value>(&text)
    {
        return saved;
    }
    serde_json::Map::new()
}

/// 读取插件设置（掩码版，供前端回显）：普通字段明文，secret 字段打掩码。
pub fn read_settings_masked(id: &str, schema: &[SettingField]) -> Value {
    let raw = read_settings(id, schema);
    let saved = load_saved_map(id);
    let mut out = serde_json::Map::new();
    for flat in expand_schema(schema, &saved) {
        let v = raw.get(&flat.key).cloned().unwrap_or_default();
        let shown = if flat.field.field_type == SettingFieldType::Secret {
            let s = v.as_str().unwrap_or("");
            if s.is_empty() {
                Value::String(String::new())
            } else {
                Value::String(mask_secret(s))
            }
        } else {
            v
        };
        out.insert(flat.key.clone(), shown);
    }
    Value::Object(out)
}

/// 保存插件设置：仅保留 schema 声明的 key（Group 按实例展开），逐字段类型校验；
/// secret 字段提交空/掩码视为"未修改"保留原值，`__clear.<key>: true` 显式清除。
/// 返回合并后的完整设置。
pub fn save_settings(id: &str, schema: &[SettingField], values: &Value) -> Result<Value, String> {
    let current = read_settings(id, schema);
    let mut out = serde_json::Map::new();
    let submitted = values.as_object().ok_or("settings 必须是对象")?;

    // 显式清除标记：__clear.<key> = true → 该 secret 字段重置为空
    let clears: std::collections::HashSet<String> = submitted
        .keys()
        .filter_map(|k| k.strip_prefix("__clear.").map(String::from))
        .collect();

    // 实例数以提交值为准（前端添加了新实例会提交 customN.*）
    for flat in expand_schema(schema, submitted) {
        let f = &flat.field;
        let key = &flat.key;
        let value = if clears.contains(key) {
            // 显式清除：恢复为空（密钥默认值）
            Value::String(String::new())
        } else {
            match submitted.get(key) {
                Some(v) => {
                    // 密钥字段：提交空字符串或与掩码完全相等 = 未修改，保留原值
                    // （前端只见掩码提示不回传明文；掩码前后追加内容会作为新值校验保存）
                    if f.field_type == SettingFieldType::Secret {
                        let s = v.as_str().ok_or_else(|| format!("{key}: 应为字符串"))?;
                        let cur = current.get(key).and_then(Value::as_str).unwrap_or("");
                        if s.is_empty() || s == mask_secret(cur) {
                            current.get(key).cloned().unwrap_or_default()
                        } else {
                            validate_value(f, v).map_err(|e| format!("{key}: {e}"))?
                        }
                    } else {
                        validate_value(f, v).map_err(|e| format!("{key}: {e}"))?
                    }
                }
                None => current.get(key).cloned().unwrap_or_default(),
            }
        };
        out.insert(key.clone(), value);
    }
    let file = settings_file(id);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建插件设置目录失败: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&Value::Object(out.clone()))
        .map_err(|e| format!("序列化设置失败: {e}"))?;
    fs::write(&file, json).map_err(|e| format!("写入设置失败: {e}"))?;
    // 密钥明文只留在本地文件：Unix 下收紧权限
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&file, fs::Permissions::from_mode(0o600));
    }
    Ok(Value::Object(out))
}

/// 按字段类型校验并归一化一个提交值。
fn validate_value(field: &SettingField, v: &Value) -> Result<Value, String> {
    match field.field_type {
        SettingFieldType::Boolean => match v {
            Value::Bool(_) => Ok(v.clone()),
            Value::String(s) if s == "true" => Ok(Value::Bool(true)),
            Value::String(s) if s == "false" => Ok(Value::Bool(false)),
            _ => Err("应为布尔值".into()),
        },
        SettingFieldType::Number => {
            // 前端删空输入框提交空字符串 → 回退字段默认值
            if v.as_str() == Some("") {
                let default = field
                    .default
                    .as_ref()
                    .and_then(|d| d.as_i64())
                    .unwrap_or(0);
                return Ok(Value::Number(default.into()));
            }
            let n = v.as_i64().ok_or("应为整数")?;
            if let Some(min) = field.min
                && n < min {
                    return Err(format!("不能小于 {min}"));
                }
            if let Some(max) = field.max
                && n > max {
                    return Err(format!("不能大于 {max}"));
                }
            Ok(Value::Number(n.into()))
        }
        SettingFieldType::Select => {
            let s = v.as_str().ok_or("应为字符串")?;
            if !field.options.iter().any(|o| o == s) {
                return Err(format!("只能是 {}", field.options.join(" / ")));
            }
            Ok(Value::String(s.to_string()))
        }
        SettingFieldType::String => {
            let s = v.as_str().ok_or("应为字符串")?;
            Ok(Value::String(s.to_string()))
        }
        SettingFieldType::Secret => {
            let s = v.as_str().ok_or("应为字符串")?;
            Ok(Value::String(s.to_string()))
        }
        // Group 是模板不是叶子，展开后不会走到这里
        SettingFieldType::Group => Err("分组字段不参与校验".into()),
    }
}

/// 密钥掩码：前 4 字符 + `****`（短密钥整体掩码）。
pub fn mask_secret(value: &str) -> String {
    if value.len() <= 4 {
        "****".to_string()
    } else {
        format!("{}****", &value[..4])
    }
}

/// 提交值是否为掩码形式（"未修改"哨兵）。粗判：以 `****` 结尾。
/// 精确判定（等于某个原值的掩码）在 save_settings 内用 `s == mask_secret(cur)` 完成——
/// 掩码前后追加内容的提交会被当作新值校验保存，而不是静默丢弃。
pub fn is_masked(value: &str) -> bool {
    value.ends_with("****")
}

#[cfg(test)]
mod tests {
    use super::*;
    // 与 config::TEST_ENV_LOCK 串行：config 测试会改全局 BOENMIND_HOME，
    // 而本模块的路径（plugins_dir）读该环境变量，并行时会读到跳变路径。
    use crate::config::TEST_ENV_LOCK;
    use std::path::PathBuf;

    /// 把 BOENMIND_HOME 切到临时目录并返回恢复函数：
    /// 本模块测试读写插件设置目录，不隔离会污染真实 `~/.boenmind/extensions/`。
    fn isolate_home() -> (PathBuf, impl FnOnce()) {
        let original = std::env::var_os("BOENMIND_HOME");
        let dir = std::env::temp_dir().join(format!(
            "bm-plugin-settings-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        unsafe { std::env::set_var("BOENMIND_HOME", &dir) };
        let restore = move || match original {
            Some(v) => unsafe { std::env::set_var("BOENMIND_HOME", v) },
            None => unsafe { std::env::remove_var("BOENMIND_HOME") },
        };
        (dir, restore)
    }

    fn test_schema() -> Vec<SettingField> {
        serde_json::from_value(serde_json::json!([
            {"key": "search.mode", "type": "select", "options": ["quick", "deep"], "default": "quick"},
            {"key": "search.cacheTtlSeconds", "type": "number", "min": 0, "max": 86400, "default": 600},
            {"key": "sources.jina.enabled", "type": "boolean", "default": true},
            {"key": "sources.jina.apiKey", "type": "secret", "default": ""},
        ]))
        .unwrap()
    }

    #[test]
    fn parse_schema_from_manifest() {
        let manifest = serde_json::json!({
            "schema": "pi.ext.manifest.v1",
            "extension_id": "web-search",
            "settings": [
                {"key": "search.mode", "type": "select", "options": ["quick", "deep"], "default": "quick"}
            ]
        });
        let schema = parse_settings_schema(&manifest).unwrap();
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].key, "search.mode");
        assert!(parse_settings_schema(&serde_json::json!({"name": "x"})).is_none());
    }

    #[test]
    fn read_merges_defaults() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (home, restore) = isolate_home();
        let id = "test-plugin-read"; // 独立 id：与并行测试互不干扰
        let _ = fs::remove_dir_all(plugins_dir().join(id)); // 清理测试产生的插件目录
        let v = read_settings(id, &test_schema());
        assert_eq!(v["search.mode"], "quick");
        assert_eq!(v["search.cacheTtlSeconds"], 600);
        assert_eq!(v["sources.jina.enabled"], true);
        restore();
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn save_roundtrip_and_masking() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (home, restore) = isolate_home();
        let id = "test-plugin-save"; // 独立 id：与并行测试互不干扰
        let schema = test_schema();
        let _ = fs::remove_dir_all(plugins_dir().join(id));
        // 保存密钥明文
        let saved = save_settings(
            id,
            &schema,
            &serde_json::json!({
                "search.mode": "deep",
                "sources.jina.apiKey": "sk-secret-123",
            }),
        )
        .unwrap();
        assert_eq!(saved["search.mode"], "deep");
        assert_eq!(saved["sources.jina.apiKey"], "sk-secret-123");
        // 读取打掩码（供前端回显）
        let with_mask: Value = {
            let raw = read_settings(id, &schema);
            let mut obj = serde_json::Map::new();
            for f in &schema {
                let v = raw.get(&f.key).cloned().unwrap_or_default();
                let shown = if f.field_type == SettingFieldType::Secret {
                    let s = v.as_str().unwrap_or("");
                    if s.is_empty() {
                        Value::String(String::new())
                    } else {
                        Value::String(mask_secret(s))
                    }
                } else {
                    v
                };
                obj.insert(f.key.clone(), shown);
            }
            Value::Object(obj)
        };
        assert_eq!(with_mask["sources.jina.apiKey"], "sk-s****");
        // 提交与掩码完全相等 = 未修改，保留原值
        let again = save_settings(
            id,
            &schema,
            &serde_json::json!({"sources.jina.apiKey": "sk-s****"}),
        )
        .unwrap();
        assert_eq!(again["sources.jina.apiKey"], "sk-secret-123");
        // 提交空字符串 = 未修改（旧语义"空 = 清除"已废弃），保留原值
        let empty = save_settings(id, &schema, &serde_json::json!({"sources.jina.apiKey": ""})).unwrap();
        assert_eq!(empty["sources.jina.apiKey"], "sk-secret-123");
        // 掩码前后追加内容 = 视为新值保存（而不是静默丢弃）
        let appended = save_settings(
            id,
            &schema,
            &serde_json::json!({"sources.jina.apiKey": "sk-s****EXTRA"}),
        )
        .unwrap();
        assert_eq!(appended["sources.jina.apiKey"], "sk-s****EXTRA");
        // 显式清除：__clear.<key> = true
        let cleared = save_settings(
            id,
            &schema,
            &serde_json::json!({"__clear.sources.jina.apiKey": true}),
        )
        .unwrap();
        assert_eq!(cleared["sources.jina.apiKey"], "");
        // 类型校验拒绝
        assert!(save_settings(id, &schema, &serde_json::json!({"search.cacheTtlSeconds": "abc"})).is_err());
        assert!(save_settings(id, &schema, &serde_json::json!({"search.mode": "ultra"})).is_err());
        let _ = fs::remove_dir_all(plugins_dir().join(id));
        restore();
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn mask_logic() {
        assert_eq!(mask_secret("abcd"), "****");
        assert_eq!(mask_secret("sk-long-key-123"), "sk-l****");
        assert!(is_masked("sk-l****"));
        // 掩码中间追加/前插不算是掩码形式（精确判定在 save 内用全等比较）
        assert!(!is_masked("sk-l****EXTRA"));
        assert!(!is_masked("sk-long-key-123"));
    }

    #[test]
    fn parse_quota_and_test_sources() {
        let manifest = serde_json::json!({
            "quota": {
                "path": ".boenmind/web-search/quota.json",
                "countOnTest": ["tavily", "exa"]
            },
            "testSources": {
                "jina": {
                    "method": "get",
                    "url": "https://s.jina.ai/?q=test",
                    "label": "Jina",
                    "headers": { "Authorization": "Bearer {sources.jina.apiKey}" }
                },
                "custom*": {
                    "method": "GET",
                    "url": "{customN.url}",
                    "headers": { "{customN.apiKeyHeader}": "{customN.apiKey}" }
                }
            }
        });
        let quota = parse_quota_decl(&manifest).unwrap();
        assert_eq!(quota.path, ".boenmind/web-search/quota.json");
        assert_eq!(quota.count_on_test, vec!["tavily", "exa"]);
        // 缺 quota 段 → None
        assert!(parse_quota_decl(&serde_json::json!({})).is_none());

        let sources = parse_test_sources(&manifest).unwrap();
        // 精确源：method 大写归一 + label 缺省补 id
        assert_eq!(sources["jina"].method, "GET");
        assert_eq!(sources["jina"].label.as_deref(), Some("Jina"));
        assert_eq!(
            sources["jina"].headers["Authorization"],
            "Bearer {sources.jina.apiKey}"
        );
        // 通配源：未声明 label 时补源 id
        assert_eq!(sources["custom*"].label.as_deref(), Some("custom*"));
        // 非法模板（缺 url）被剔除
        let bad = serde_json::json!({ "testSources": { "x": { "method": "GET" } } });
        assert!(parse_test_sources(&bad).is_none());
        // 缺 testSources 段 → None
        assert!(parse_test_sources(&serde_json::json!({})).is_none());
    }

    #[test]
    fn group_expands_and_persists() {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (home, restore) = isolate_home();
        let id = "test-plugin-group"; // 独立 id：与并行测试互不干扰
        let _ = fs::remove_dir_all(plugins_dir().join(id));
        let schema: Vec<SettingField> = serde_json::from_value(serde_json::json!([
            {
                "key": "custom*",
                "type": "group",
                "instances": 2,
                "fields": [
                    {"key": "enabled", "type": "boolean", "default": false},
                    {"key": "name", "type": "string", "default": ""},
                    {"key": "url", "type": "string", "default": ""},
                    {"key": "apiKey", "type": "secret", "default": ""}
                ]
            }
        ]))
        .unwrap();

        // 默认 2 实例展开
        let v = read_settings(id, &schema);
        assert_eq!(v["custom1.enabled"], false);
        assert_eq!(v["custom2.name"], "");
        assert!(v.get("custom3.enabled").is_none());

        // 保存含 custom3 的值（模拟前端添加实例）
        let saved = save_settings(
            id,
            &schema,
            &serde_json::json!({
                "custom1.enabled": true,
                "custom1.name": "mysearx",
                "custom1.apiKey": "sk-custom-1",
                "custom3.name": "third",
                "custom3.url": "https://t.example.com/search?q={query}",
            }),
        )
        .unwrap();
        assert_eq!(saved["custom1.name"], "mysearx");
        assert_eq!(saved["custom1.apiKey"], "sk-custom-1");
        // 实际实例数扩展到 3
        assert_eq!(saved["custom3.name"], "third");
        // 掩码版：custom1.apiKey 掩码、custom3 也可见
        let masked = read_settings_masked(id, &schema);
        assert_eq!(masked["custom1.apiKey"], "sk-c****");
        assert_eq!(masked["custom3.name"], "third");
        // 掩码提交保留
        let again = save_settings(id, &schema, &serde_json::json!({"custom1.apiKey": "sk-c****"})).unwrap();
        assert_eq!(again["custom1.apiKey"], "sk-custom-1");
        let _ = fs::remove_dir_all(plugins_dir().join(id));
        restore();
        let _ = fs::remove_dir_all(&home);
    }
}
