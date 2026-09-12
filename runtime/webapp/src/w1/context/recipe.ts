//! Prompt 配方解析(自 context.tsx 机械移入;纯函数)。
import type { CtxStep } from "../../w2/api";
import type { ParsedPromptRecipe, FileSideEffect } from "./utils";
import { estTokens } from "./utils";

export function parseStepRecipe(step: CtxStep): ParsedPromptRecipe {
  let rawSystemPrompt = "";
  let personaText = "";
  const skills: Array<{ id: string; name: string; instruction: string }> = [];
  let workspaceText: string | null = null;
  const historyTurns: Array<{ turnIndex: number; user: string; assistant: string }> = [];
  let currentUserInput = "";
  const affectedFiles: FileSideEffect[] = [];
  let reasoningSnippet: string | null = null;

  const messages = step.messages ?? [];

  // 1. 解析 System Prompt
  const sysMsg = messages.find((m) => m.role === "system");
  if (sysMsg && sysMsg.content) {
    rawSystemPrompt = sysMsg.content;
    // ADR-0056:优先直读快照的结构化 system_parts(persona/技能/工作目录)——
    // 后端渲染 prompt 时一并落盘,前端不再正则反解析 [附加技能]/[工作目录] 标记。
    const parts = step.system_parts;
    if (parts && (parts.persona != null || (parts.skills?.length ?? 0) > 0 || parts.workspace)) {
      personaText = (parts.persona ?? "").trim();
      (parts.skills ?? []).forEach((s, i) => {
        skills.push({ id: `skill_${i}`, name: s.name, instruction: s.instruction });
      });
      workspaceText = parts.workspace ? `[工作目录] ${parts.workspace}` : null;
    } else {
      // 回退:旧快照无 system_parts,按 prompt 文本标记解析(逐步淘汰)。
      let raw = sysMsg.content;
      const wsIdx = raw.indexOf("[工作目录]");
      if (wsIdx !== -1) {
        workspaceText = raw.substring(wsIdx).trim();
        raw = raw.substring(0, wsIdx).trim();
      }
      const skillRegex = /\[附加技能 · ([^\]]+)\]\n([\s\S]*?)(?=\n\n\[附加技能|\n\n$|$)/g;
      let match: RegExpExecArray | null;
      const firstSkillIdx = raw.indexOf("[附加技能 · ");
      if (firstSkillIdx !== -1) {
        personaText = raw.substring(0, firstSkillIdx).trim();
        let sIdx = 0;
        while ((match = skillRegex.exec(raw)) !== null) {
          skills.push({
            id: `skill_${sIdx++}`,
            name: match[1].trim(),
            instruction: match[2].trim(),
          });
        }
      } else {
        personaText = raw.trim();
      }
    }
  }

  // 2. 解析历史与当前提问
  const nonSys = messages.filter((m) => m.role === "user" || m.role === "assistant");
  if (nonSys.length > 0) {
    const last = nonSys[nonSys.length - 1];
    if (last.role === "user") {
      currentUserInput = last.content;
      const prev = nonSys.slice(0, nonSys.length - 1);
      let tCount = 1;
      for (let i = 0; i < prev.length; i += 2) {
        const u = prev[i]?.role === "user" ? prev[i].content : "";
        const a = prev[i + 1]?.role === "assistant" ? prev[i + 1].content : "";
        if (u || a) {
          historyTurns.push({ turnIndex: tCount++, user: u, assistant: a });
        }
      }
    }
  }

  // 3. 解析工具箱(ADR-0055:不再由描述文案反推审批语义——工具表快照不含
  // 审批事实,如实只展示名称与描述)
  const toolList = (step.tools ?? []).map((t: any) => {
    const fn = t.function ?? {};
    const name = fn.name ?? "未知工具";
    const desc = fn.description ?? "";
    const paramStr = JSON.stringify(fn.parameters ?? {});
    return {
      name,
      description: desc,
      paramTokens: estTokens(paramStr),
      rawSchema: fn.parameters,
    };
  });

  // 4. 解析文件副作用追踪与思考链
  for (const m of messages) {
    // 检查推理思考链标记
    if (m.content && (m.content.includes("<think>") || m.content.includes("thinking:"))) {
      const start = m.content.indexOf("<think>");
      const end = m.content.indexOf("</think>");
      if (start !== -1 && end !== -1) {
        reasoningSnippet = m.content.slice(start + 7, end).trim();
      }
    }
  }

  return {
    rawSystemPrompt,
    personaText: personaText || "默认通用助理",
    skills,
    workspaceText,
    historyTurns,
    currentUserInput,
    toolList,
    affectedFiles,
    reasoningSnippet,
  };
}
