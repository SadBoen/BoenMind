// 输入对照测试页(用户排查「真键盘打不了字」):三个输入框对照——
// ①纯原生 ②React 受控 ③BoenMind 同款 assistant-ui Composer(同款接线)。
// 各框实时回显值;哪个框吞字一目了然。独立入口,不进产品主包。
import { createRoot } from "react-dom/client";
import { useEffect, useState } from "react";
import {
  AssistantRuntimeProvider,
  ComposerPrimitive,
  useExternalStoreRuntime,
  useAuiState,
} from "@assistant-ui/react";

function CasePlain() {
  const [v, setV] = useState("");
  return (
    <>
      <textarea
        value={undefined}
        defaultValue=""
        onInput={(e) => setV((e.target as HTMLTextAreaElement).value)}
        placeholder="在这里打字…"
      />
      <div className="val">
        {JSON.stringify(v)}({v.length} 字)
      </div>
    </>
  );
}

function CaseReact() {
  const [v, setV] = useState("");
  return (
    <>
      <textarea
        value={v}
        onChange={(e) => setV(e.target.value)}
        placeholder="在这里打字…"
      />
      <div className="val">
        {JSON.stringify(v)}({v.length} 字)
      </div>
    </>
  );
}

function CaseAui() {
  const runtime = useExternalStoreRuntime({
    messages: [],
    setMessages: () => {},
    onNew: async () => {},
    isRunning: false,
    convertMessage: (m: unknown) => m,
  });
  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <ComposerPrimitive.Root>
        <ComposerPrimitive.Input
          placeholder="在这里打字…"
          style={{ width: "100%", minHeight: 56, font: "inherit", padding: 8, boxSizing: "border-box" }}
        />
        <ValueProbe />
      </ComposerPrimitive.Root>
    </AssistantRuntimeProvider>
  );
}

function ValueProbe() {
  const text = useAuiState((s) => {
    const c = (s as { composer?: { text?: string } }).composer;
    return c?.text ?? "";
  });
  return (
    <div className="val">
      store 里的值:{JSON.stringify(text ?? "")}({(text ?? "").length} 字)
    </div>
  );
}

function App() {
  return (
    <>
      <CasePlain />
      <CaseReact />
      <CaseAui />
    </>
  );
}

createRoot(document.getElementById("case-plain")!).render(<CasePlain />);
createRoot(document.getElementById("case-react")!).render(<CaseReact />);
createRoot(document.getElementById("case-aui")!).render(<CaseAui />);
