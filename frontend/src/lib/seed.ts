import type { CatalogItem, FileNode } from "../types";

export const SEED_FILES: FileNode[] = [
  {
    id: "d-docs",
    name: "docs",
    path: "docs",
    kind: "dir",
    children: [
      {
        id: "f-guide",
        name: "FRONTEND-GUIDE.md",
        path: "docs/FRONTEND-GUIDE.md",
        kind: "text",
        content: "# BoenMind 前端 UI 指导文件\n\ntoken 先行。rail 48px。文件入口只有 topbar「文件」。\n",
      },
      {
        id: "f-handoff",
        name: "FRONTEND-HANDOFF.md",
        path: "docs/FRONTEND-HANDOFF.md",
        kind: "text",
        content: "按 v3 一口气重写。不要 dockview。\n",
      },
    ],
  },
  {
    id: "d-src",
    name: "src",
    path: "src",
    kind: "dir",
    children: [
      {
        id: "f-app",
        name: "App.tsx",
        path: "src/App.tsx",
        kind: "text",
        content: "export default function App() {\n  return <Shell />;\n}\n",
      },
      {
        id: "f-bin",
        name: "logo.bin",
        path: "src/logo.bin",
        kind: "binary",
      },
      {
        id: "f-img",
        name: "mark.svg",
        path: "src/mark.svg",
        kind: "image",
        content:
          '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="#3b82f6"/><text x="12" y="42" fill="#fff" font-size="28">B</text></svg>',
      },
    ],
  },
  {
    id: "f-readme",
    name: "README.md",
    path: "README.md",
    kind: "text",
    content: "# BoenMind\n\n本地 agent 平台。\n",
  },
];

export const SEED_SKILLS: CatalogItem[] = [
  { id: "sk-chat", name: "聊天", type: "内置", builtin: true, enabled: true, config: { 启用: true } },
  { id: "sk-files", name: "文件", type: "内置", builtin: true, enabled: true, config: { 启用: true } },
  {
    id: "sk-review",
    name: "代码评审",
    type: "第三方",
    builtin: false,
    enabled: true,
    config: { 严格模式: true, 语言: "中文" },
  },
];

export const SEED_PLUGINS: CatalogItem[] = [
  { id: "pg-core", name: "核心", type: "内置", builtin: true, enabled: true, config: { 启用: true } },
  {
    id: "pg-browser",
    name: "浏览器",
    type: "第三方",
    builtin: false,
    enabled: true,
    config: { 无头: false },
  },
];
