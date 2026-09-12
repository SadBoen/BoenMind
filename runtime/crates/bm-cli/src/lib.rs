//! bm-cli:Surface 客户端库。`boenmind` 二进制是薄壳,真正的协议逻辑在这里,
//! 供端到端测试(M3 规格 T5)与未来脚本化复用。

use bm_contract::error_codes::ErrorCode;
use bm_contract::ids::IdGen;
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

/// Wire API 客户端(bearer 令牌在构造时注入)。
pub struct EnvelopeClient {
    url: String,
    http: reqwest::blocking::Client,
    id_gen: bm_contract::ids::UlidIdGen,
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
        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            http,
            id_gen: bm_contract::ids::UlidIdGen,
        })
    }

    /// 调用一个 Wire 方法:信封逐字节,ok=true 返回 result,ok=false 返回 Envelope 错误。
    pub fn call(&self, method: Method, params: Value) -> Result<Value, CallError> {
        let envelope = RequestEnvelope::new(method, self.id_gen.next_id("req"), params);
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

// ---- 离线自检:独立 Judge 评估(M8.7;#57 裁决=CLI 装开关)------------------

/// 缺省数据目录(与 boenmind-server `--data-dir` 缺省同源)。
/// 平台默认数据目录(单源在 bm-core;ADR-0046 P5 收口)。
pub use bm_persist::default_data_dir;

/// 对 `<data-dir>` 的事件日志跑独立评估器(离线只读,不经 server;
/// 评估器为确定性:同区间恒同报告)。`from_seq`/`to_seq` 缺省 = 全量区间。
/// 返回合同形态 evaluation-report.v0_1;打开失败/空日志/区间非法 = Err(用户可读)。
pub fn run_judge(
    data_dir: &std::path::Path,
    from_seq: Option<u64>,
    to_seq: Option<u64>,
) -> Result<serde_json::Value, String> {
    use bm_persist::EventStore as _;
    let store = bm_persist::PersistStore::open(data_dir)
        .map_err(|e| format!("打开持久层失败({}): {e}", data_dir.display()))?;
    let last = store
        .last_log_seq()
        .map_err(|e| format!("读取日志末尾失败: {e}"))?;
    if last == 0 {
        return Err("事件日志为空,无账可查".into());
    }
    let from = from_seq.unwrap_or(1);
    let to = to_seq.unwrap_or(last);
    if from == 0 || from > to {
        return Err(format!(
            "区间非法: [{from},{to}](须 1 ≤ from ≤ to ≤ {last})"
        ));
    }
    bm_judge::evaluate(&store, from, to).map_err(|e| e.to_string())
}

#[cfg(test)] // 门控剥除:测试模块不进生产 lib(同步全仓 mod tests 惯例)
mod judge_tests {
    use super::*;

    /// 空日志 → 用户可读文案,而非裸错误(新装/未跑过的数据目录是常态)。
    #[test]
    fn judge_on_empty_log_is_friendly_error() {
        let dir = tempfile::tempdir().expect("临时目录");
        let err = run_judge(dir.path(), None, None).expect_err("空日志必须报错");
        assert!(err.contains("事件日志为空"), "文案须可读: {err}");
    }

    /// 区间非法(或被 --from/--to 钳出界)→ 带日志末尾的可读提示。
    #[test]
    fn judge_rejects_inverted_range() {
        use bm_contract::events::{EventEnvelope, EventType};
        use bm_persist::EventStore as _;
        let dir = tempfile::tempdir().expect("临时目录");
        let store = bm_persist::PersistStore::open(dir.path()).expect("打开");
        store
            .record(&EventEnvelope::new(
                1,
                EventType::RuntimeStarted,
                "2026-08-30T12:00:00.000Z".into(),
                None,
                None,
                None,
                serde_json::json!({"pid": 1, "version": "test", "started_at": "2026-08-30T12:00:00.000Z"}),
            ))
            .expect("写事件");
        let err = run_judge(dir.path(), Some(9), Some(3)).expect_err("倒序区间必须报错");
        assert!(err.contains("区间非法"), "文案须可读: {err}");
    }
}
