/**
 * 宿主共享终端面板（TerminalPane，架构 §四·B 补充"公共功能页组件化"第一批）：
 * 终端是"手脚"级宿主能力，任何应用可嵌入（编程壳右栏 Tab = 第一个消费者）。
 *
 * 实现 = 上游吸收（backend/vendor/UPSTREAM_TRACKING.md T1/T2）：@xterm/xterm
 * 渲染 + 后端 portable-pty 会话（/api/terminal 创建/输入/调尺寸/SSE 输出流）。
 * 一期 = 用户显式操作的终端（不触发插件权限询问，不进事件日志——模型命令
 * 可视化与审计留二期，届时走工具执行侧事件链）。
 */
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Terminal as XtermTerminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { api } from "@/api/client";

export interface TerminalPaneProps {
  /** 启动目录（缺省 = 配置工作目录）；编程壳传当前项目根 */
  cwd?: string;
}

/** base64 → Uint8Array（后端输出任意字节，含 UTF-8 多字节安全过 base64） */
function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

export function TerminalPane({ cwd }: TerminalPaneProps) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<XtermTerminal | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const closeRef = useRef<(() => void) | null>(null);
  const fitRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    // xterm 实例（VS Code 同源渲染；禁用原生滚动条防嵌套滚动条）
    const term = new XtermTerminal({
      fontFamily: "Cascadia Code, Consolas, 'Courier New', monospace",
      fontSize: 12.5,
      cursorBlink: true,
      scrollback: 5000,
      theme: { background: "#0d1117" },
    });
    termRef.current = term;
    const fit = new FitAddon();
    fitRef.current = fit;
    term.loadAddon(fit);
    term.open(container);
    fit.fit();

    let closed = false;
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;

    // 创建后端 pty 会话（cwd = 当前项目根；宽高 = 容器初始 fit 值）
    api
      .createTerminal({ cwd, cols: term.cols, rows: term.rows })
      .then(async ({ id }) => {
        if (closed) {
          void api.closeTerminal(id);
          return;
        }
        sessionIdRef.current = id;

        // 输出流订阅（SSE）：output → xterm；exit → 显示退出信息
        const close = api.subscribeTerminal(id, (ev) => {
          if (ev.type === "output" && ev.data) {
            term.write(decodeBase64(ev.data));
          } else if (ev.type === "exit") {
            term.write(`\r\n\x1b[90m[${t("terminal.exited")} ${ev.code ?? -1}]\x1b[0m\r\n`);
          }
        });
        closeRef.current = close;
      })
      .catch(() => {
        if (!closed) term.write(`\r\n\x1b[31m[${t("terminal.createFailed")}]\x1b[0m\r\n`);
      });

    // 输入：键入 → 后端 pty 写端（拆块防大帧）
    const inputDisposable = term.onData((data) => {
      const id = sessionIdRef.current;
      if (!id) return;
      void api.terminalInput(id, new TextEncoder().encode(data)).catch(() => {});
    });

    // 容器尺寸变化 → 后端 pty resize（防抖）
    const observer = new ResizeObserver(() => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        if (closed) return;
        fit.fit();
        const id = sessionIdRef.current;
        if (id) void api.terminalResize(id, term.cols, term.rows).catch(() => {});
      }, 150);
    });
    observer.observe(container);

    return () => {
      closed = true;
      if (resizeTimer) clearTimeout(resizeTimer);
      observer.disconnect();
      inputDisposable.dispose();
      closeRef.current?.();
      const id = sessionIdRef.current;
      if (id) void api.closeTerminal(id);
      term.dispose();
      termRef.current = null;
      sessionIdRef.current = null;
    };
  }, [cwd, t]);

  return <div ref={containerRef} className="h-full w-full overflow-hidden bg-[#0d1117] p-2" />;
}
