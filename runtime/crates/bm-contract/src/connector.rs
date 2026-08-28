//! 模型连接器合同镜像(model/connector.v0_1.schema.json)。
//! invoke 是 Runtime 内部调用,不走 Wire API;实现可替换,合同不破(基线 5.4)。

use crate::BmTimestamp;
use crate::error_codes::ErrorCode;
use crate::ids::BmId;
use serde::{Deserialize, Serialize};

/// `^[a-z0-9][a-z0-9-_.]{0,62}$`
pub fn validate_model_id(s: &str) -> Result<(), String> {
    let ok = !s.is_empty()
        && s.len() <= 63
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(format!("非法 model_id: {s:?}"))
    }
}

/// `^secret:[A-Za-z0-9_.-]{1,64}$`;凭据本体只存在 Secret Store(基线 4.6)。
pub fn validate_secret_ref(s: &str) -> Result<(), String> {
    let body = s.strip_prefix("secret:").ok_or("须以 secret: 开头")?;
    let ok = !body.is_empty()
        && body.len() <= 64
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok {
        Ok(())
    } else {
        Err(format!("非法 secret_ref: {s:?}(注意:字符集不含 '/')"))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDescriptor {
    pub model_id: String,
    pub provider: String,
    pub context_window: u32,
    pub max_output: u32,
    pub cost_per_1k_in: f64,
    pub cost_per_1k_out: f64,
    /// M1 恒为 false;M4 引入 Capability 后由连接器如实上报。
    pub supports_tools: bool,
    pub fallback_rank: u32,
}

wire_str_enum!(Role {
    System => "system",
    User => "user",
    Assistant => "assistant",
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct InvokeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetCtx {
    pub operation_id: BmId,
    pub agent_id: BmId,
    pub remaining_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvokeRequest {
    pub model_id: String,
    pub messages: Vec<Message>,
    /// M1 恒为空数组(合同 maxItems: 0)。
    pub tools: Vec<serde_json::Value>,
    #[serde(default)]
    pub params: InvokeParams,
    pub secret_ref: String,
    pub budget_ctx: BudgetCtx,
    pub deadline: BmTimestamp,
    /// 降级/重试链中的第几次尝试(1..=3,合同上限)。
    pub attempt: u32,
}

wire_str_enum!(FinishReason {
    Stop => "stop",
    Length => "length",
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub tokens_in: u64,
    pub tokens_out: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.tokens_in + self.tokens_out
    }
}

/// 调用结果:成败二选一,由 `ok` 字段判别。失败分支无自由文本字段——
/// 合同结构本身阻止错误消息携带敏感内容(基线 8.4)。
#[derive(Debug, Clone, PartialEq)]
pub enum InvokeResponse {
    Completed {
        content: String,
        finish_reason: FinishReason,
        usage: Usage,
        model_id: String,
        latency_ms: u64,
        stream_interrupted: bool,
    },
    Failed {
        error_code: ErrorCode,
        retryable: bool,
        attempt: u32,
        detail_ref: Option<String>,
    },
}

impl InvokeResponse {
    pub fn is_ok(&self) -> bool {
        matches!(self, InvokeResponse::Completed { .. })
    }
}

impl Serialize for InvokeResponse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        match self {
            InvokeResponse::Completed {
                content,
                finish_reason,
                usage,
                model_id,
                latency_ms,
                stream_interrupted,
            } => {
                map.serialize_entry("ok", &true)?;
                map.serialize_entry("content", content)?;
                map.serialize_entry("finish_reason", finish_reason)?;
                map.serialize_entry("usage", usage)?;
                map.serialize_entry("model_id", model_id)?;
                map.serialize_entry("latency_ms", latency_ms)?;
                map.serialize_entry("stream_interrupted", stream_interrupted)?;
            }
            InvokeResponse::Failed {
                error_code,
                retryable,
                attempt,
                detail_ref,
            } => {
                map.serialize_entry("ok", &false)?;
                map.serialize_entry("error_code", &WireCode(error_code.as_str()))?;
                map.serialize_entry("retryable", retryable)?;
                map.serialize_entry("attempt", attempt)?;
                if let Some(d) = detail_ref {
                    map.serialize_entry("detail_ref", d)?;
                }
            }
        }
        map.end()
    }
}

struct WireCode<'a>(&'a str);

impl serde::Serialize for WireCode<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for InvokeResponse {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            ok: bool,
            #[serde(default)]
            content: Option<String>,
            #[serde(default)]
            finish_reason: Option<FinishReason>,
            #[serde(default)]
            usage: Option<Usage>,
            #[serde(default)]
            model_id: Option<String>,
            #[serde(default)]
            latency_ms: Option<u64>,
            #[serde(default)]
            stream_interrupted: Option<bool>,
            #[serde(default)]
            error_code: Option<String>,
            #[serde(default)]
            retryable: Option<bool>,
            #[serde(default)]
            attempt: Option<u32>,
            #[serde(default)]
            detail_ref: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        if raw.ok {
            Ok(InvokeResponse::Completed {
                content: raw
                    .content
                    .ok_or_else(|| serde::de::Error::missing_field("content"))?,
                finish_reason: raw
                    .finish_reason
                    .ok_or_else(|| serde::de::Error::missing_field("finish_reason"))?,
                usage: raw
                    .usage
                    .ok_or_else(|| serde::de::Error::missing_field("usage"))?,
                model_id: raw
                    .model_id
                    .ok_or_else(|| serde::de::Error::missing_field("model_id"))?,
                latency_ms: raw
                    .latency_ms
                    .ok_or_else(|| serde::de::Error::missing_field("latency_ms"))?,
                stream_interrupted: raw.stream_interrupted.unwrap_or(false),
            })
        } else {
            let code_str = raw
                .error_code
                .ok_or_else(|| serde::de::Error::missing_field("error_code"))?;
            let code = ErrorCode::from_wire(&code_str)
                .ok_or_else(|| serde::de::Error::custom(format!("未知错误码: {code_str}")))?;
            Ok(InvokeResponse::Failed {
                error_code: code,
                retryable: raw.retryable.unwrap_or(false),
                attempt: raw.attempt.unwrap_or(1),
                detail_ref: raw.detail_ref,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_ref_charset() {
        assert!(validate_secret_ref("secret:model.zhipu").is_ok());
        assert!(validate_secret_ref("secret:openai.gpt-4o-mini_1").is_ok());
        // 黄金轨迹示例中的 '/' 不在合同字符集内(规格 §8.3),以 schema 为准。
        assert!(validate_secret_ref("secret:model/zhipu").is_err());
        assert!(validate_secret_ref("model.zhipu").is_err());
    }

    #[test]
    fn model_id_charset() {
        assert!(validate_model_id("zhipu.glm-4-flash").is_ok());
        assert!(validate_model_id("-zhipu").is_err());
    }
}
