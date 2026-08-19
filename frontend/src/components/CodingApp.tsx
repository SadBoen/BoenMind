import { DockviewReact, DockviewReadyEvent } from "dockview-react";
import { useState } from "react";
import { Button, Input, Tree, Typography } from "antd";
import { CaretDownOutlined, CaretRightOutlined, SendOutlined } from "@ant-design/icons";
import HeaderActions from "./HeaderActions";

// 编程 APP（轻量 AI IDE）：dockview 六区布局。
// 左 Explorer（文件树）/ 中编辑器（欢迎页+多 tab）/ 右 AI 对话 / 下 终端·输出·问题 / 底状态栏。

export default function CodingApp() {
  const [model] = useState("mock-1");

  const onReady = (event: DockviewReadyEvent) => {
    const api = event.api;
    // 中央编辑器（主导区）：欢迎页 + 示例文件 tab（堆叠）
    const editor = api.addPanel({ id: "editor", component: "welcome", title: "欢迎" });
    api.addPanel({
      id: "file-main", component: "file", title: "main.rs",
      position: { referencePanel: editor, direction: "within" },
    });
    // 左文件树
    api.addPanel({
      id: "explorer", component: "explorer", title: "资源管理器",
      position: { referencePanel: editor, direction: "left" },
    });
    // 右 AI 对话
    api.addPanel({
      id: "ai", component: "ai", title: "AI",
      position: { referencePanel: editor, direction: "right" },
    });
    // 底部 终端/输出/问题
    api.addPanel({
      id: "bottom", component: "bottom", title: "终端 · 输出 · 问题",
      position: { referencePanel: editor, direction: "below" },
    });
  };

  return (
    <div className="coding-app">
      <div className="coding-main">
        <DockviewReact
          onReady={onReady}
          rightHeaderActionsComponent={HeaderActions}
          components={{
            welcome: () => <EditorWelcome />,
            file: () => <EditorFile />,
            explorer: () => <Explorer />,
            ai: () => <AIPanel model={model} />,
            bottom: () => <BottomPanel />,
          }}
        />
      </div>
      <StatusBar model={model} />
    </div>
  );
}

// ---- 资源管理器（antd Tree，示例结构）----
const TREE = [
  { title: "src", key: "src", children: [
    { title: "main.rs", key: "src/main.rs", icon: "🦀" },
    { title: "lib.rs", key: "src/lib.rs", icon: "🦀" },
    { title: "api", key: "src/api", children: [
      { title: "mod.rs", key: "src/api/mod.rs", icon: "🦀" },
      { title: "rpc.rs", key: "src/api/rpc.rs", icon: "🦀" },
    ]},
  ]},
  { title: "Cargo.toml", key: "Cargo.toml", icon: "📦" },
  { title: "README.md", key: "README.md", icon: "📄" },
];

function Explorer() {
  return (
    <div className="explorer">
      <div className="explorer-header">资源管理器</div>
      <div className="explorer-tree">
        <Tree
          showIcon
          defaultExpandAll
          treeData={TREE}
          switcherIcon={({ expanded }) =>
            expanded ? <CaretDownOutlined /> : <CaretRightOutlined />
          }
        />
      </div>
    </div>
  );
}

// ---- 编辑器欢迎页（新建项目/打开文件夹/AI 新建 三卡片）----
function EditorWelcome() {
  return (
    <div className="editor-welcome">
      <h1>BoenMind Code</h1>
      <p className="editor-welcome-sub">开始你的编程之旅</p>
      <div className="welcome-cards">
        <Button className="welcome-card" type="text">
          <span className="welcome-card-icon">📁</span>
          <span className="welcome-card-title">新建项目</span>
          <span className="welcome-card-desc">AI 脚手架搭建</span>
        </Button>
        <Button className="welcome-card" type="text">
          <span className="welcome-card-icon">🗂️</span>
          <span className="welcome-card-title">打开文件夹</span>
          <span className="welcome-card-desc">浏览本地工作区</span>
        </Button>
        <Button className="welcome-card" type="text">
          <span className="welcome-card-icon">🤖</span>
          <span className="welcome-card-title">AI 新建</span>
          <span className="welcome-card-desc">对话生成项目</span>
        </Button>
      </div>
    </div>
  );
}

// ---- 编辑器文件 tab（占位代码区）----
const SAMPLE_CODE = `// main.rs — 示例文件
fn main() {
    println!("Hello, BoenMind!");
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
`;

function EditorFile() {
  const [text] = useState(SAMPLE_CODE);
  return (
    <div className="editor-file">
      <pre className="editor-code">{text}</pre>
      <div className="editor-ln">
        {text.split("\n").map((_, i) => (
          <div key={i}>{i + 1}</div>
        ))}
      </div>
    </div>
  );
}

// ---- AI 对话面板 ----
function AIPanel({ model }: { model: string }) {
  const [msgs, setMsgs] = useState<{ role: string; text: string }[]>([
    { role: "assistant", text: "我是你的编程助手。选中代码问我，或直接描述需求。" },
  ]);
  const [input, setInput] = useState("");
  const send = () => {
    if (!input.trim()) return;
    setMsgs((m) => [...m, { role: "user", text: input.trim() }]);
    setInput("");
    setTimeout(() => {
      setMsgs((m) => [...m, { role: "assistant", text: "（AI 回复占位——接入 agent 后在此流式返回）" }]);
    }, 300);
  };
  return (
    <div className="ai-panel">
      <div className="ai-messages">
        {msgs.map((m, i) => (
          <div key={i} className={`ai-msg ai-msg-${m.role}`}>
            <span className="ai-msg-label">{m.role === "user" ? "我" : "AI"}</span>
            <span className="ai-msg-text">{m.text}</span>
          </div>
        ))}
      </div>
      <div className="ai-composer">
        <Input.TextArea
          rows={2}
          value={input}
          placeholder="问 AI…"
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); } }}
        />
        <div className="ai-composer-footer">
          <span className="ai-model">{model}</span>
          <Button
            className="chat-send-btn"
            type="primary"
            shape="circle"
            icon={<SendOutlined rotate={-45} />}
            disabled={!input.trim()}
            onClick={send}
          />
        </div>
      </div>
    </div>
  );
}

// ---- 底部面板（终端/输出/问题）----
function BottomPanel() {
  const [tab, setTab] = useState<"terminal" | "output" | "problems">("terminal");
  return (
    <div className="bottom-panel">
      <div className="bottom-tabs">
        {(["terminal", "output", "problems"] as const).map((t) => (
          <button key={t} className={`bottom-tab ${tab === t ? "active" : ""}`} onClick={() => setTab(t)}>
            {t === "terminal" ? "终端" : t === "output" ? "输出" : "问题"}
          </button>
        ))}
      </div>
      <div className="bottom-body">
        {tab === "terminal" && <pre className="terminal-output">$ cargo run{`\n   Compiling boenmind v0.1.0\n    Finished dev [unoptimized + debuginfo]\n     Running target/debug/web-server.exe\n`}</pre>}
        {tab === "output" && <div className="muted" style={{ padding: 12 }}>构建输出区（占位）</div>}
        {tab === "problems" && (
          <div className="problems-list">
            <div className="problem problem-error">⨯ main.rs:12:5 — 未使用的变量 `x`（占位）</div>
            <div className="problem problem-warn">⚠ lib.rs:3:1 — 函数从未被使用（占位）</div>
          </div>
        )}
      </div>
    </div>
  );
}

// ---- 状态栏 ----
function StatusBar({ model }: { model: string }) {
  return (
    <div className="status-bar">
      <div className="status-left">
        <span className="status-item">⎇ main</span>
        <span className="status-item status-err">⨯ 1</span>
        <span className="status-item status-warn">⚠ 1</span>
      </div>
      <div className="status-right">
        <span className="status-item">{model}</span>
        <span className="status-item">🦀 Rust</span>
        <span className="status-item">Ln 1, Col 1</span>
      </div>
    </div>
  );
}
