//! bm-cli:Surface 客户端库。`boenmind` 二进制是薄壳,真正的协议逻辑在这里,
//! 供端到端测试(M3 规格 T5)与未来脚本化复用。

use bm_contract::error_codes::ErrorCode;
use bm_contract::ids::{BmId, IdGen};
use bm_contract::wire::{Method, RequestEnvelope, ResponseEnvelope};
use serde_json::Value;

/// 调用失败(信封 ok=false)。exit_code 来自错误码注册表 cli_exit。
#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error("传输失败: {0}")]
    Transport(String),
    #[error("{message}")]
    Envelope { code: ErrorCode, message: String },
}

impl CallError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CallError::Transport(_) => 7, // unavailable 对应
            CallError::Envelope { code, .. } => code.cli_exit(),
        }
    }

    pub fn error_object(&self) -> Value {
        match self {
            CallError::Transport(m) => serde_json::json!({"code": "unavailable", "message": m}),
            CallError::Envelope { code, message } => {
                serde_json::json!({"code": code.as_str(), "message": message})
            }
        }
    }
}

type IdGenBox = Box<dyn Fn(&str) -> BmId + Send>;

/// Wire API 客户端(bearer 令牌在构造时注入)。
pub struct EnvelopeClient {
    url: String,
    http: reqwest::blocking::Client,
    id_gen: IdGenBox,
}

pub fn default_token_path() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("boenmind")
        .join("token")
}

impl EnvelopeClient {
    /// `token = None` 时读默认令牌文件。
    pub fn new(url: &str, token: Option<&str>) -> Result<Self, String> {
        let token = match token {
            Some(t) => t.to_string(),
            None => std::fs::read_to_string(default_token_path())
                .map_err(|e| format!("读取令牌文件失败({e});可用 --token-file 或 --token 指定"))?
                .trim()
                .to_string(),
        };
        if token.is_empty() {
            return Err("令牌为空".into());
        }
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| format!("令牌头非法: {e}"))?,
        );
        let http = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("客户端构建失败: {e}"))?;
        let generator = bm_contract::ids::UlidIdGen;
        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            http,
            id_gen: Box::new(move |prefix| IdGen::next_id(&generator, prefix)),
        })
    }

    /// 调用一个 Wire 方法:信封逐字节,ok=true 返回 result,ok=false 返回 Envelope 错误。
    pub fn call(&self, method: Method, params: Value) -> Result<Value, CallError> {
        let envelope = RequestEnvelope::new(method, (self.id_gen)("req"), params);
        let r = self
            .http
            .post(format!("{}/rpc/{}", self.url, method.as_str()))
            .json(&envelope)
            .send()
            .map_err(|e| CallError::Transport(format!("传输失败: {e}")))?;
        let status = r.status().as_u16();
        let body: Value = r
            .json()
            .map_err(|e| CallError::Transport(format!("响应非 JSON: {e}")))?;
        match status {
            200 => match serde_json::from_value::<ResponseEnvelope>(body)
                .map_err(|e| CallError::Transport(format!("信封解析失败: {e}")))?
            {
                ResponseEnvelope::Success { result, .. } => Ok(result),
                ResponseEnvelope::Failure { error, .. } => Err(CallError::Envelope {
                    code: error.code.get(),
                    message: error.message,
                }),
            },
            400 => Err(CallError::Transport(format!("请求信封被拒: {body}"))),
            401 => Err(CallError::Transport("鉴权失败(401):令牌缺失或错误".into())),
            404 => Err(CallError::Transport("未知方法路径".into())),
            other => Err(CallError::Transport(format!("HTTP {other}: {body}"))),
        }
    }

    /// watch(M3.3):SSE 增量流,原始帧直接写 stdout(可 grep)。
    /// Ctrl-C 终止;断线由用户以 --since 重连(resume cursor 语义)。
    pub fn watch(&self, session_id: &str, since_seq: u64) -> Result<(), String> {
        let mut r = self
            .http
            .get(format!("{}/events/{session_id}", self.url))
            .query(&[("since_seq", since_seq.to_string())])
            .send()
            .map_err(|e| format!("watch 连接失败: {e}"))?;
        let status = r.status().as_u16();
        if status != 200 {
            let body = r.text().unwrap_or_default();
            return Err(format!("watch HTTP {status}: {body}"));
        }
        let mut stdout = std::io::stdout();
        r.copy_to(&mut stdout)
            .map_err(|e| format!("流读取失败: {e}"))?;
        Ok(())
    }
}
