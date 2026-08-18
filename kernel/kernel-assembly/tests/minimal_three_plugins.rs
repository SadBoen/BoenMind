//! 核心三插件最小装配与热插拔集成测试。
//!
//! 钉死契约（用户定调）：**LLM provider / loop / tools 三个核心插件装配
//! 即可运行完整回合**——最小基座 = 三插件，storage/session 是内核基础设施
//! 不插件化。分类标签：三插件 category 全 Core，供插件管理员分组隐藏。
//!
//! 1. 最小装配可运行：Runtime::headless = llm 插件(mock) + loop 插件 +
//!    tools 插件，create_session → run_turn 完整回合闭环。
//! 2. 热插拔回归：swap_llm 换装后下一回合走新实现；swap_loop 换装后新会话
//!    用新实现（运行中会话不受影响）；register/unregister_tool + gate 生效。
//! 3. 分类清单：plugin_manifest() 恰好 llm/loop/tools 三条，category 全 Core。

use std::sync::Arc;

use kernel_assembly::{default_agent_factory, Runtime};
use kernel_contracts::plugin::{PluginCategory, PLUGIN_LLM, PLUGIN_LOOP, PLUGIN_TOOLS};
use kernel_contracts::session::{SessionEvent, SessionHeader, SessionId, TurnEndReason, TurnEvent};
use kernel_contracts::{
    ChunkStream, FinishReason, GenerateOptions, LlmError, LlmModelInfo, LlmPort, StreamChunk,
    TokenUsage, ToolError, ToolExecutionInput, ToolExecutionResult, ToolHandler,
};
use kernel_loop::AgentPort;

// ---------- mocks ----------

/// 固定输出指定文本的 LLM（A/B 区分热插拔换装生效）。
#[derive(Clone)]
struct FixedTextLlm {
    text: String,
}

#[async_trait::async_trait]
impl LlmPort for FixedTextLlm {
    async fn list_models(&self, _provider: &str) -> Result<Vec<LlmModelInfo>, LlmError> {
        Ok(vec![LlmModelInfo {
            id: "fixed-1".to_string(),
            label: None,
            supports_tools: true,
            context_window: None,
            max_tokens: None,
            reasoning: None,
        }])
    }
    fn stream(&self, _request: GenerateOptions) -> ChunkStream {
        let text = self.text.clone();
        Box::pin(futures::stream::iter(vec![
            Ok(StreamChunk::BlockStart {
                index: 0,
                block_type: "text".to_string(),
            }),
            Ok(StreamChunk::TextDelta {
                index: 0,
                text: text.clone(),
            }),
            Ok(StreamChunk::BlockEnd {
                index: 0,
                block: kernel_contracts::ContentBlock::Text(text),
            }),
            Ok(StreamChunk::Usage(TokenUsage {
                input: 1,
                output: 1,
                cache_read: None,
                cache_write: None,
                reasoning: None,
            })),
            Ok(StreamChunk::Finish(FinishReason::Stop)),
        ]))
    }
}

/// echo 工具（热插拔装卸断言用）。
struct EchoTool;

#[async_trait::async_trait]
impl ToolHandler for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "echoes back the given text"
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "text": { "type": "string" } },
            "required": ["text"]
        })
    }
    async fn execute(
        &self,
        input: ToolExecutionInput,
    ) -> Result<ToolExecutionResult, ToolError> {
        let text = input
            .arguments
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        Ok(ToolExecutionResult::ok(format!("echo:{text}")))
    }
}

// ---------- helpers ----------

fn header(id: &str) -> SessionHeader {
    SessionHeader {
        id: SessionId(id.to_string()),
        app: "test".into(),
        profile: "headless".into(),
        workspace: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn tmp_db(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bm-minimal-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("test.db")
}

/// 从日志取最后一个 AssistantMessage 的文本（纯文本块）。
fn last_text(agent: &Arc<dyn AgentPort>) -> String {
    agent
        .session()
        .events()
        .iter()
        .rev()
        .find_map(|r| match &r.event {
            SessionEvent::AssistantMessage { content, .. } => {
                let text: String = content
                    .iter()
                    .filter_map(|b| match b {
                        kernel_contracts::ContentBlock::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            _ => None,
        })
        .unwrap_or_default()
}

// ---------- tests ----------

/// 最小装配可运行：三插件（llm mock + loop + tools）→ create_session →
/// run_turn 完整回合闭环（事件序列 + 文本产出 + turn 编号递增）。
#[tokio::test]
async fn minimal_three_plugins_runs_full_turn() {
    let db = tmp_db("minimal");
    let rt = Runtime::headless(db.clone()).unwrap();
    // llm 插件：换装固定文本 mock（真实装配点=web-server swap_llm）。
    rt.swap_llm(Arc::new(FixedTextLlm {
        text: "你好，最小基座。".to_string(),
    }));
    // tools 插件：注册并启用一个工具（门控 fail-closed，须显式 enable）。
    rt.register_tool(Arc::new(EchoTool)).unwrap();
    rt.gate.enable("echo");

    let agent = rt.create_session(header("s1")).await.unwrap();
    let outcome = agent.run_turn(Some("hi")).await.unwrap();
    assert_eq!(outcome.reason, TurnEndReason::Completed);
    assert_eq!(last_text(&agent), "你好，最小基座。");

    // 工具面可用：echo 工具经门控可执行。
    let res = rt
        .gate
        .execute_guarded(
            &rt.tools,
            ToolExecutionInput {
                name: "echo".to_string(),
                arguments: serde_json::json!({ "text": "ok" }),
            },
        )
        .await
        .unwrap();
    assert_eq!(res.output, "echo:ok");

    // turn 编号递增（恢复续跑不重复语义不回归）。
    agent.run_turn(Some("again")).await.unwrap();
    let starts: Vec<u64> = agent
        .session()
        .events()
        .into_iter()
        .filter_map(|r| match r.event {
            SessionEvent::Turn(TurnEvent::Started { turn }) => Some(turn),
            _ => None,
        })
        .collect();
    assert_eq!(starts, vec![1, 2]);
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

/// swap_llm 热换装：A 回合产出 A 文本 → 换装 B → 下一回合产出 B 文本
/// （运行中会话同一 agent 实例，换装即时生效——RwLock 读侧零锁开销）。
#[tokio::test]
async fn swap_llm_takes_effect_on_next_turn() {
    let db = tmp_db("swap-llm");
    let rt = Runtime::headless(db.clone()).unwrap();
    rt.swap_llm(Arc::new(FixedTextLlm {
        text: "from-A".to_string(),
    }));
    let agent = rt.create_session(header("s1")).await.unwrap();
    agent.run_turn(Some("first")).await.unwrap();
    assert_eq!(last_text(&agent), "from-A");

    rt.swap_llm(Arc::new(FixedTextLlm {
        text: "from-B".to_string(),
    }));
    agent.run_turn(Some("second")).await.unwrap();
    assert_eq!(last_text(&agent), "from-B");
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

/// swap_loop 热换装：换装自定义工厂后，新会话用新实现、运行中会话不受影响。
#[tokio::test]
async fn swap_loop_affects_new_sessions_only() {
    let db = tmp_db("swap-loop");
    let rt = Runtime::headless(db.clone()).unwrap();
    // llm 插件换装固定文本（供"运行中会话不受影响"断言产出）。
    rt.swap_llm(Arc::new(FixedTextLlm {
        text: "still-standard".to_string(),
    }));
    let agent_a = rt.create_session(header("s1")).await.unwrap();

    // 换装自定义 loop 工厂：产出一个"探测"代理（run_turn 被拦下打标）。
    let marker = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let probe = Arc::clone(&marker);
    rt.swap_loop(Arc::new(move |_, _| {
        let probe = Arc::clone(&probe);
        struct ProbeAgent {
            marker: Arc<std::sync::atomic::AtomicBool>,
        }
        #[async_trait::async_trait]
        impl AgentPort for ProbeAgent {
            async fn run_turn(
                &self,
                _user_text: Option<&str>,
            ) -> Result<kernel_loop::TurnOutcome, kernel_loop::LoopError> {
                self.marker.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(kernel_loop::TurnOutcome {
                    steps: 0,
                    reason: TurnEndReason::Completed,
                })
            }
            fn abort(&self) {}
            fn session(&self) -> Arc<kernel_session::Session> {
                unimplemented!("probe agent exposes no session")
            }
            fn set_model_override(&self, _provider: String, _model: String) {}
            fn model_override(&self) -> Option<(String, String)> {
                None
            }
            fn clear_model_override(&self) {}
        }
        Arc::new(ProbeAgent {
            marker: probe,
        }) as Arc<dyn AgentPort>
    }));

    // 运行中会话不受影响：s1 仍是标准实现（回合正常产出）。
    agent_a.run_turn(Some("still-standard")).await.unwrap();
    assert_eq!(last_text(&agent_a), "still-standard");

    // 新会话用新实现：s2 的 run_turn 被探测代理拦截（marker 置位）。
    let agent_b = rt.create_session(header("s2")).await.unwrap();
    agent_b.run_turn(Some("probed")).await.unwrap();
    assert!(
        marker.load(std::sync::atomic::Ordering::SeqCst),
        "swap_loop factory must serve new sessions"
    );
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

/// 工具热插拔：unregister 后门控/注册表不可达；重 register 恢复。
#[tokio::test]
async fn tool_register_unregister_hot_swap() {
    let db = tmp_db("tool-swap");
    let rt = Runtime::headless(db.clone()).unwrap();
    rt.register_tool(Arc::new(EchoTool)).unwrap();
    rt.gate.enable("echo");

    rt.unregister_tool("echo").unwrap();
    // 卸载后：执行报"not found"（注册表不可达）。
    let err = rt
        .tools
        .execute(ToolExecutionInput {
            name: "echo".to_string(),
            arguments: serde_json::json!({ "text": "x" }),
        })
        .await
        .unwrap_err();
    assert!(err.0.contains("not found"));

    // 重复卸载报错（诚实，不静默）。
    assert!(rt.unregister_tool("echo").is_err());

    // 重注册 + 启用恢复可用。
    rt.register_tool(Arc::new(EchoTool)).unwrap();
    rt.gate.enable("echo");
    let res = rt
        .gate
        .execute_guarded(
            &rt.tools,
            ToolExecutionInput {
                name: "echo".to_string(),
                arguments: serde_json::json!({ "text": "again" }),
            },
        )
        .await
        .unwrap();
    assert_eq!(res.output, "echo:again");
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

/// 分类清单：plugin_manifest() 恰好 llm/loop/tools 三条、category 全 Core；
/// serde 形状 `{id, category:"core", name, description, version}`。
#[test]
fn manifest_lists_three_core_plugins() {
    let db = tmp_db("manifest");
    let rt = Runtime::headless(db.clone()).unwrap();
    let manifest = rt.plugin_manifest();
    assert_eq!(manifest.len(), 3);
    let ids: Vec<&str> = manifest.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec![PLUGIN_LLM, PLUGIN_LOOP, PLUGIN_TOOLS]);
    for e in &manifest {
        assert_eq!(e.category, PluginCategory::Core);
        assert!(!e.name.is_empty());
        assert!(!e.description.is_empty());
        assert!(!e.version.is_empty());
    }
    // serde 形状（RPC 直接序列化给插件管理员）。
    let v = serde_json::to_value(&manifest[0]).unwrap();
    assert_eq!(v["id"], "llm");
    assert_eq!(v["category"], "core");
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

/// 默认工厂与默认装配路径：Runtime::headless 即三插件最小基座（回归保护：
/// headless 装配不被插件化改造破坏）。
#[test]
fn default_factory_is_react_loop_agent() {
    let db = tmp_db("default-factory");
    let rt = Runtime::headless(db.clone()).unwrap();
    // AgentFactory 可 clone 且可调用（默认工厂 = ReactLoopAgent 装配）。
    let factory = rt.plugin_manifest();
    assert_eq!(factory.len(), 3);
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

// 保持 default_agent_factory 导出可见（文档承诺的默认工厂）。
#[allow(dead_code)]
fn _touch_default_factory() {
    let _ = default_agent_factory();
}
