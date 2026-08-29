//! boenmind:Surface CLI(M3.2)。薄壳——协议逻辑在 bm-cli 库。
//!
//! 命令组:session / agent / operations / events 全量;task / approval
//! 随 M4/M5 增发(对象尚不存在,预留子命令位,M3 规格 §1)。

use bm_cli::EnvelopeClient;
use bm_contract::wire::Method;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "boenmind",
    version,
    about = "BoenMind Surface CLI(连接 boenmind-server)"
)]
struct Cli {
    /// 服务端地址
    #[arg(long, default_value = "http://127.0.0.1:7531")]
    url: String,
    /// 访问令牌(缺省读令牌文件)
    #[arg(long)]
    token: Option<String>,
    /// 令牌文件路径(--token 缺省时的读取来源)
    #[arg(long, default_value_t = bm_cli::default_token_path().display().to_string())]
    token_file: String,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 会话
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },
    /// Agent
    Agent {
        #[command(subcommand)]
        cmd: AgentCmd,
    },
    /// 执行收据
    Operations {
        #[command(subcommand)]
        cmd: OpsCmd,
    },
    /// 事件(watch = SSE 流,Ctrl-C 终止)
    Events {
        #[command(subcommand)]
        cmd: EventsCmd,
    },
    /// 任务(M4 提供)
    Task,
    /// 审批(M4 提供)
    Approval,
}

#[derive(Subcommand)]
enum SessionCmd {
    /// 创建会话(绑定一个 Agent)
    Create {
        #[arg(long, default_value = "assistant")]
        name: String,
        #[arg(long, default_value = "zhipu.glm-4-flash")]
        model: String,
        #[arg(long, default_value_t = 100_000)]
        max_tokens: u64,
        #[arg(long, default_value_t = 20)]
        max_turns: u32,
    },
    /// 恢复会话(补发 since 之后事件)
    Resume {
        session_id: String,
        #[arg(long, default_value_t = 0)]
        since: u64,
    },
    /// 关闭会话(进行中回合不被取消,INV-6)
    Close {
        session_id: String,
        #[arg(long, default_value = "user_request")]
        reason: String,
    },
}

#[derive(Subcommand)]
enum AgentCmd {
    /// 发起回合(默认即返;--wait 阻塞至终态;CLI 退出不取消任务)
    Send {
        session_id: String,
        agent_id: String,
        content: String,
        #[arg(long, default_value = "trusted")]
        input_trust: String,
        #[arg(long, default_value_t = 30)]
        wait_timeout: u64,
        #[arg(long)]
        no_wait: bool,
    },
    /// 显式取消(唯一 cancelled 入口,INV-12)
    Cancel {
        session_id: String,
        agent_id: String,
        operation_id: String,
    },
}

#[derive(Subcommand)]
enum OpsCmd {
    /// 查询执行收据(终态后幂等,INV-9)
    Get { operation_id: String },
}

#[derive(Subcommand)]
enum EventsCmd {
    /// 拉取事件(增量)
    Poll {
        session_id: String,
        #[arg(long, default_value_t = 0)]
        since: u64,
        #[arg(long, default_value_t = 100)]
        limit: u32,
    },
    /// 订阅事件流(SSE,Ctrl-C 终止;断线以 --since 重连)
    Watch {
        session_id: String,
        #[arg(long, default_value_t = 0)]
        since: u64,
    },
}

#[derive(Subcommand)]
enum TaskCmd {
    /// 占位:M4 提供
    List,
}

#[derive(Subcommand)]
enum ApprovalCmd {
    /// 占位:M4 提供
    List,
}

fn main() {
    let cli = Cli::parse();
    // 令牌优先级:--token 显式 > --token-file > 默认路径(库内处理)
    let token: Option<String> = match cli.token {
        Some(t) => Some(t),
        None => std::fs::read_to_string(&cli.token_file)
            .ok()
            .map(|t| t.trim().to_string()),
    };
    let client = EnvelopeClient::new(&cli.url, token.as_deref()).unwrap_or_else(|e| fail(&e, 2));

    let out: Result<serde_json::Value, bm_cli::CallError> = match cli.command {
        Cmd::Session { cmd } => match cmd {
            SessionCmd::Create {
                name,
                model,
                max_tokens,
                max_turns,
            } => client.call(
                Method::SessionCreate,
                serde_json::json!({"agent": {"name": name, "model_chain": [model],
                    "budget": {"max_tokens": max_tokens, "max_turns": max_turns}}}),
            ),
            SessionCmd::Resume { session_id, since } => client.call(
                Method::SessionResume,
                serde_json::json!({"session_id": session_id, "since_seq": since}),
            ),
            SessionCmd::Close { session_id, reason } => client.call(
                Method::SessionClose,
                serde_json::json!({"session_id": session_id, "reason": reason}),
            ),
        },
        Cmd::Agent { cmd } => match cmd {
            AgentCmd::Send {
                session_id,
                agent_id,
                content,
                input_trust,
                wait_timeout,
                no_wait,
            } => {
                let trust = match input_trust.as_str() {
                    "trusted" => bm_contract::wire::InputTrust::Trusted,
                    other => fail(&format!("input_trust 仅支持 trusted(M1 合同): {other}"), 2),
                };
                let receipt = client.call(
                    Method::AgentSendInput,
                    serde_json::json!({"session_id": session_id, "agent_id": agent_id,
                        "content": content, "input_trust": trust.as_str()}),
                );
                match receipt {
                    Ok(r) if no_wait => Ok(r),
                    Ok(receipt) => {
                        // --wait(默认):阻塞至终态(轮询收据;CLI 断开不影响服务端)
                        let op = receipt["operation_id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();
                        let deadline = std::time::Instant::now()
                            + std::time::Duration::from_secs(wait_timeout.max(1));
                        let mut final_receipt = receipt;
                        let mut settled = false;
                        loop {
                            match client.call(
                                Method::OperationsGet,
                                serde_json::json!({"operation_id": op}),
                            ) {
                                Ok(rr) => {
                                    if rr["state"].as_str().map(|s| {
                                        matches!(
                                            s,
                                            "succeeded"
                                                | "failed"
                                                | "cancelled"
                                                | "timeout"
                                                | "outcome_unknown"
                                        )
                                    }) == Some(true)
                                    {
                                        final_receipt = rr;
                                        settled = true;
                                        break;
                                    }
                                }
                                Err(e) => fail(&e.to_string(), e.exit_code()),
                            }
                            if std::time::Instant::now() > deadline {
                                println!(
                                    "--wait 超时({wait_timeout}s),回合仍在服务端进行;收据可随时 operations get {op} 查询"
                                );
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        let _ = settled;
                        Ok(final_receipt)
                    }
                    Err(e) => Err(e),
                }
            }
            AgentCmd::Cancel {
                session_id,
                agent_id,
                operation_id,
            } => client.call(
                Method::AgentCancel,
                serde_json::json!({"session_id": session_id, "agent_id": agent_id,
                    "operation_id": operation_id}),
            ),
        },
        Cmd::Operations { cmd } => match cmd {
            OpsCmd::Get { operation_id } => client.call(
                Method::OperationsGet,
                serde_json::json!({"operation_id": operation_id}),
            ),
        },
        Cmd::Events { cmd } => match cmd {
            EventsCmd::Poll {
                session_id,
                since,
                limit,
            } => client.call(
                Method::EventsPoll,
                serde_json::json!({"session_id": session_id, "since_seq": since, "limit": limit}),
            ),
            EventsCmd::Watch { session_id, since } => {
                if let Err(e) = client.watch(&session_id, since) {
                    fail(&e, 7);
                }
                std::process::exit(0);
            }
        },
        Cmd::Task => {
            println!("task 命令组随 M4(Task 对象)提供;当前里程碑范围见基线 §18-M3");
            return;
        }
        Cmd::Approval => {
            println!("approval 命令组随 M4(Approval 对象)提供;当前里程碑范围见基线 §18-M3");
            return;
        }
    };

    print_result(&out, "rpc");
}

fn print_result(out: &Result<serde_json::Value, bm_cli::CallError>, _ctx: &str) {
    match out {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(v).expect("序列化"));
        }
        Err(e) => {
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&e.error_object()).expect("序列化")
            );
            std::process::exit(e.exit_code());
        }
    }
}

fn fail(msg: &str, code: i32) -> ! {
    eprintln!("{msg}");
    std::process::exit(code);
}
