export type ViewId = "chat" | "code" | "wiki" | "settings";
export type StyleAxis = "modern" | "cartoon";
export type MaterialAxis = "solid" | "glass";
export type BgType = "solid" | "image";
export type FontSize = "sm" | "md" | "lg";
export type ReasoningLevel = "off" | "low" | "medium" | "high";
export type ThinkingDisplay = "auto" | "expanded" | "hidden";
export type Language = "zh" | "en";
export type ProviderKind = "minimax" | "deepseek" | "custom";
export type ProviderShape = "openai-compatible" | "anthropic" | "gemini";

export interface ProviderConfig {
  id: string;
  name: string;
  kind: ProviderKind;
  shape?: ProviderShape;
  base_url?: string;
  api_key?: string;
  models: string[];
  default_model?: string;
}

export interface Session {
  id: string;
  title: string;
  preview: string;
  updatedAt: number;
  tags: string[];
  archived: boolean;
  /** 后端会话 busy 标记（host/session-status 推送；不存在 = false）。 */
  running?: boolean;
}

export type MsgRole = "user" | "assistant";

export interface ToolCall {
  id: string;
  name: string;
  summary: string;
  detail?: string;
  status: "running" | "done" | "error";
}

export interface Attachment {
  id: string;
  name: string;
  size: number;
  type: string;
}

/** 后端 approval/requested 帧的待审批项（应答走 POST /api/respond）。 */
export interface PendingApproval {
  rpcId: string;
  sessionId: string;
  approvalId: string;
  toolName: string;
  callId?: string;
  reason?: string;
}

export interface ChatMessage {
  id: string;
  role: MsgRole;
  content: string;
  createdAt: number;
  model?: string;
  thinking?: string;
  tools?: ToolCall[];
  attachments?: Attachment[];
  compressed?: boolean;
}

export type FileKind = "dir" | "text" | "image" | "binary";

export interface FileNode {
  id: string;
  name: string;
  path: string;
  kind: FileKind;
  children?: FileNode[];
  content?: string;
  dataUrl?: string;
}

export interface CatalogItem {
  id: string;
  name: string;
  type: string;
  builtin: boolean;
  enabled: boolean;
  config: Record<string, string | boolean>;
}

export interface Settings {
  language: Language;
  fontSize: FontSize;
  defaultModel: string;
  defaultReasoning: ReasoningLevel;
  providers: ProviderConfig[];
  defaultProvider: string;
  style: StyleAxis;
  material: MaterialAxis;
  background: BgType;
  /** 背景图片地址（URL 或 dataURL；为空回落到内置春天绿叶图） */
  bgUrl: string;
  glassG: number;
  glassHue: number;
  thinkingDisplay: ThinkingDisplay;
}

export interface AppState {
  view: ViewId;
  returnView: ViewId;
  sessions: Session[];
  activeSessionId: string | null;
  messages: Record<string, ChatMessage[]>;
  sessionCollapsed: boolean;
  fileDockCollapsed: boolean;
  editorCollapsed: boolean;
  narrow: boolean;
  connected: boolean;
  streaming: boolean;
  streamStartedAt: number | null;
  /** 首次后端探测完成（出现会话列表/空态前）。 */
  booted: boolean;
  /** 后端装配的真实 provider 模型目录（llm.models；与设置提供商合并为聊天下拉事实源）。 */
  backendModels: Array<{ provider: string; models: string[] }>;
  settings: Settings;
  selectedTags: string[];
  sessionSearch: string;
  composer: string;
  composerAttachments: Attachment[];
  model: string;
  reasoning: ReasoningLevel;
  usage: { used: number; limit: number };
  /** 待审批队列（approval/requested；首项弹审批卡，应答后移除）。 */
  pendingApprovals: PendingApproval[];
  files: FileNode[];
  cwd: string[];
  openFilePath: string | null;
  openFileDraft: string;
  openFileDirty: boolean;
  skills: CatalogItem[];
  plugins: CatalogItem[];
  contextMenu: { x: number; y: number; filePath: string } | null;
  confirm: {
    title: string;
    body: string;
    confirmLabel?: string;
    danger?: boolean;
    onConfirm: () => void;
    extraLabel?: string;
    onExtra?: () => void;
  } | null;
  modal: { title: string; item: CatalogItem; kind: "skill" | "plugin" } | null;
  thinkingOverrides: Record<string, boolean>;
}

export const DEFAULT_SETTINGS: Settings = {
  language: "zh",
  fontSize: "md",
  defaultModel: "grok-4.6",
  defaultReasoning: "medium",
  providers: [],
  defaultProvider: "",
  style: "modern",
  material: "solid",
  background: "solid",
  bgUrl: "",
  glassG: 0.42,
  glassHue: 0,
  thinkingDisplay: "auto",
};
