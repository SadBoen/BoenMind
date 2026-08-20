import { useEffect, useRef, useState } from "react";
import { Button, Select, Input, Tooltip } from "antd";
import { PaperClipOutlined, SoundOutlined, TranslationOutlined } from "@ant-design/icons";
import { ArrowUp, CircleArrowUp, Send } from "lucide-react";
import { useChat } from "../hooks/useChat";
import { useCurrentSession } from "../sessionStore";
import { getPresetId } from "../theme";
import { rpc } from "../client";
import MessageList from "./MessageList";

// 发送按钮图标按风格档换肤（Grok 处方：黑白=向上箭头 / 卡通=纸飞机 / 玻璃=圆环箭头）
function SendButtonIcon() {
  const preset = getPresetId();
  if (preset === "cartoon") return <Send size={18} strokeWidth={2.25} />;
  if (preset === "glass") return <CircleArrowUp size={16} strokeWidth={1.5} />;
  return <ArrowUp size={16} strokeWidth={2} />;
}

// 模型目录（session.models wire 形状）
interface ModelGroup {
  id: string;
  name: string;
  models: { id: string; name: string }[];
}

// 思考档位（对齐老前端 ChatInput 默认四档；按选中模型动态出档后置）
const DEFAULT_THINKING = ["off", "low", "medium", "high"] as const;

// 聊天单元右侧面板：消息流 + 老前端（ChatInput）风格 composer。
// 输入框 = 圆角卡片；工具条在卡片内下边缘（提示在左，附件/语言/语音/模型/思考/发送靠右）。
export default function ChatPanel() {
  const [text, setText] = useState("");
  const sessionId = useCurrentSession();
  const { messages, streaming, error, send } = useChat(sessionId);

  // 模型选择
  const [groups, setGroups] = useState<ModelGroup[]>([]);
  const [current, setCurrent] = useState("mock-1");
  // 思考档位（占位；后端动态档位后置）
  const [thinking, setThinking] = useState<string>("medium");

  useEffect(() => {
    if (!sessionId) return;
    rpc<{ current: { provider: string; model: string }; groups: ModelGroup[] }>(
      "session.models", { sessionId }
    )
      .then((v) => {
        setGroups(v.groups ?? []);
        setCurrent(v.current?.model || v.groups?.[0]?.models?.[0]?.name || "mock-1");
      })
      .catch(() => {});
  }, [sessionId]);

  const pickModel = async (provider: string, model: string, name: string) => {
    setCurrent(name);
    if (!sessionId) return;
    try {
      await rpc("session.selectModel", { sessionId, provider, model });
    } catch { /* advisory */ }
  };

  // textarea 自动增高（老前端同款：rows=1 起始，max-h-40）
  const taRef = useRef<HTMLTextAreaElement>(null);
  const autoGrow = () => {
    const ta = taRef.current;
    if (!ta) return;
    ta.style.height = "auto";
    ta.style.height = Math.min(ta.scrollHeight, 160) + "px";
  };

  const canSend = text.trim().length > 0 && !streaming;
  const submit = () => {
    if (!canSend || !sessionId) return;
    send(text.trim());
    setText("");
    requestAnimationFrame(() => {
      const ta = taRef.current;
      if (ta) ta.style.height = "auto";
    });
  };

  // antd Select 模型选项（按 provider 分组）
  const modelOptions = groups.map((g) => ({
    label: g.name,
    options: g.models.map((m) => ({ value: `${g.id}::${m.id}::${m.name}`, label: m.name })),
  }));

  return (
    <div className="chat-panel">
      <MessageList messages={messages} />
      {error && <div className="chat-error">{error}</div>}
      {/* 输入区（对齐老前端 ChatInput）：圆角卡片 + 卡片内下边缘工具条 */}
      <div className="chat-input-wrap">
        <div className={`chat-input-card ${canSend ? "can-send" : ""}`}>
          <Input.TextArea
            ref={taRef as React.Ref<React.ComponentRef<typeof Input.TextArea>>}
            rows={1}
            autoSize={{ minRows: 1, maxRows: 6 }}
            value={text}
            placeholder={sessionId ? "输入消息，Enter 发送 / Shift+Enter 换行" : "请先选择或新建会话"}
            disabled={!sessionId || streaming}
            onChange={(e) => { setText(e.target.value); autoGrow(); }}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                submit();
              }
            }}
            className="chat-composer"
          />
          {/* 卡片内下边缘工具条：提示在左，附件/语言/语音/模型/思考/发送靠右 */}
          <div className="chat-input-toolbar">
            <span className="chat-input-hint">
              {text.length > 0 ? `${text.length} 字符` : "Enter 发送 / Shift+Enter 换行"}
            </span>
            <div className="chat-input-actions">
              {/* 占位按钮（hermes-webui 参考，功能后接） */}
              <Tooltip title="附件（待实现）">
                <Button className="chat-tool-btn" type="text" disabled icon={<PaperClipOutlined />} />
              </Tooltip>
              <Tooltip title="常用语言（待实现）">
                <Button className="chat-tool-btn" type="text" disabled icon={<TranslationOutlined />} />
              </Tooltip>
              <Tooltip title="语音（待实现）">
                <Button className="chat-tool-btn" type="text" disabled icon={<SoundOutlined />} />
              </Tooltip>

              {/* 模型选择（antd Select，分组） */}
              <Select
                className="chat-model-select"
                size="small"
                variant="borderless"
                value={current}
                options={modelOptions}
                popupMatchSelectWidth={false}
                onChange={(val) => {
                  const [provider, model, name] = val.split("::");
                  pickModel(provider, model, name);
                }}
                placeholder="选择模型"
                suffixIcon={<span className="chat-select-chevron">▾</span>}
              />

              {/* 思考档位（antd Select） */}
              <Select
                className="chat-thinking-select"
                size="small"
                variant="borderless"
                value={thinking}
                options={DEFAULT_THINKING.map((v) => ({ value: v, label: v }))}
                popupMatchSelectWidth={false}
                onChange={setThinking}
                prefix={<span className="chat-select-icon">🧠</span>}
              />

              {/* 发送按钮（图标随风格档：黑白↑ / 卡通✈ / 玻璃⭯） */}
              <Button
                className="chat-send-btn"
                type="primary"
                shape="circle"
                icon={<SendButtonIcon />}
                title="发送"
                disabled={!canSend}
                onClick={submit}
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
