/**
 * 聊天窗口（全窗形态的宿主共享 ChatPane 包装）。
 * 对话界面 = 宿主能力（架构 §四·B 补充 v0.22）：实现主体已抽至 ChatPane，
 * 本组件保留全窗语义（标题栏含 selectOrCreate 引导），编程壳等应用嵌入
 * 时直接用 ChatPane 的 panel 形态，不重复实现对话界面。
 */
import { ChatPane } from "./ChatPane";

export function ChatWindow() {
  return <ChatPane variant="full" />;
}
