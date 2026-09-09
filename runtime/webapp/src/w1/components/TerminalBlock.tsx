import { useState } from "react";
import { ToolActionItem } from "../parser";

interface TerminalBlockProps {
  item: ToolActionItem;
}

/**
 * 终端命令行(2026-09-10 对齐 ZCode 样式,废除卡片/仿终端黑块):
 * 灰色"终端 <命令>"通栏单行,超宽截断出省略号;点击展开执行回执
 * (W1 数据面只有命令本身,无输出流,按原口径提示已下发)。
 * 蓝本 token 见 .zcode/zcode-chat-replica.html(离线复刻留档)
 */
export function TerminalBlock({ item }: TerminalBlockProps) {
  const [open, setOpen] = useState(false);
  const command = item.target || item.name;

  return (
    <div>
      <div
        className="zc-t clickable zc-term"
        data-slot="terminal-head"
        title={command}
        onClick={() => setOpen(!open)}
      >
        <span className="lbl">终端</span>
        <span className="zc-term-cmd">{command}</span>
      </div>
      {open ? (
        <div className="zc-sub">
          <div className="zc-term-out">
            <div>$ {command}</div>
            <div className="zc-term-out-note">... 命令已发送给环境执行 ...</div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
