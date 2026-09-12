// 输入区 Composer(#22 拆分:自 thread.tsx 机械移入):
// 输入框 + 角色/模型/工作目录/思考等级/权限模式五个选择器 + 发送/停止。
import { ComposerPrimitive, useAuiState } from "@assistant-ui/react";
import {
  FolderOpen,
  Send,
  ShieldCheck,
  Square,
  Zap,
  ListOrdered,
  Brain,
  Cpu,
} from "lucide-react";
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from "react";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
} from "@/components/ui/select";
import { api, type WorkspaceEntry } from "@/w2/api";
import { storage, STORAGE_KEYS, type PermissionMode, type ThinkingLevel } from "@/lib/storage";
import { BM_EVENTS, emit, on } from "../../lib/bus";

/** 新建对话等场景的命令式入口(thread.tsx 持 ref 调用,替代跨文件 DOM 扒取) */
export interface ComposerHandle {
  focusAndClear: () => void;
}

export const Composer = forwardRef<ComposerHandle>(function Composer(_props, ref) {
  const isRunning = useAuiState((s) => s.thread.isRunning);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  useImperativeHandle(
    ref,
    () => ({
      focusAndClear: () => {
        const el = inputRef.current;
        if (el) {
          el.value = "";
          el.focus();
        }
      },
    }),
    [],
  );
  const [model, setModel] = useState("…");
  // W6:对话级模型选择——候选 = 各提供商「常用」并集(设置→模型 勾选);
  // 选择持久化 localStorage,随每条消息发给后端,中途切换下一条即生效。
  const [modelGroups, setModelGroups] = useState<Array<{ provider: string; models: string[] }>>([]);
  const [selModel, setSelModel] = useState<string>(
    () => storage.get(STORAGE_KEYS.ACTIVE_MODEL) || "",
  );
  const [roles, setRoles] = useState<Array<{ id: string; name: string }>>([]);
  const [activeRole, setActiveRole] = useState<string>(
    () => storage.get(STORAGE_KEYS.ACTIVE_ROLE) || "",
  );
  // W8:对话工作目录选择——候选 = 注册表(设置→常规 维护);选择持久化
  // localStorage 并随每条消息发给后端,中途切换下一条即生效(模型同款)。
  const [workspaces, setWorkspaces] = useState<Array<WorkspaceEntry>>([]);
  const [selWorkspace, setSelWorkspace] = useState<string>(
    () => storage.get(STORAGE_KEYS.ACTIVE_WORKSPACE) || "",
  );
  // 权限模式(ADR-0030):状态权威在服务端会话,本组件只是选择器。
  // 新对话(会话尚未创建)时选值暂存本地作"新会话意向",会话创建后自动
  // 上报一次;有会话后切换即发指令改服务端状态,裁决在服务端执行。
  const [permMode, setPermMode] = useState<PermissionMode>("ask");
  // 思考等级选择: off(关闭)|low(轻度)|medium(中度)|high(深度)
  const [thinkingLevel, setThinkingLevel] = useState<ThinkingLevel>(
    () => (storage.get(STORAGE_KEYS.THINKING_LEVEL) as ThinkingLevel) || "medium",
  );

  const loadWorkspaces = () => {
    api.workspaces
      .list()
      .then((d) => {
        const list = d?.workspaces ?? [];
        setWorkspaces(list);
        // 已选工作区被删除 → 回落默认(不绑定)
        const cur = storage.get(STORAGE_KEYS.ACTIVE_WORKSPACE) || "";
        if (cur && !list.some((w) => w.id === cur)) {
          storage.remove(STORAGE_KEYS.ACTIVE_WORKSPACE);
          setSelWorkspace("");
        }
      })
      // P1-30:下拉加载失败不再静默,console 告警
      .catch((e) => console.warn("工作目录列表加载失败", e));
  };

  const loadModels = () => {
    api.providers.list()
      .then((d) => {
        const list: Array<{ provider: string; models: string[] }> = (d?.providers ?? [])
          .filter((p: any) => (p.modelsCommon ?? []).length > 0)
          .map((p: any) => ({ provider: p.name, models: p.modelsCommon as string[] }));
        setModelGroups(list);
        // 已选模型不在候选集(被取消勾选/删除)→ 回落服务器默认
        const all = new Set(list.flatMap((g: any) => g.models));
        const cur = storage.get(STORAGE_KEYS.ACTIVE_MODEL) || "";
        if (cur && !all.has(cur)) {
          storage.remove(STORAGE_KEYS.ACTIVE_MODEL);
          setSelModel("");
          emit(BM_EVENTS.activeModelChanged);
        }
      })
      .catch((e) => console.warn("模型候选列表加载失败", e));
  };

  const loadRoles = () => {
    api.roles.get()
      .then((d) => {
        if (d.roles && Array.isArray(d.roles)) {
          setRoles(d.roles);
          const current = storage.get(STORAGE_KEYS.ACTIVE_ROLE);
          if (!current || !d.roles.some((r: any) => r.id === current)) {
            const next = d.active_id || d.roles[0]?.id || "assistant";
            setActiveRole(next);
            storage.set(STORAGE_KEYS.ACTIVE_ROLE, next);
          } else {
            setActiveRole(current);
          }
        }
      })
      .catch((e) => console.warn("角色列表加载失败", e));
  };

  useEffect(() => {
    // P1-30:401 时正向跳登录由统一 client 处理(与 runtime.tsx 主流同口径)
    api.models()
      .then((v) => setModel(v?.data?.[0]?.id ?? "?"))
      .catch(() => setModel("?"));

    loadModels();
    loadRoles();
    loadWorkspaces();
    window.addEventListener(BM_EVENTS.rolesChanged, loadRoles);
    window.addEventListener(BM_EVENTS.providersChanged, loadModels);
    window.addEventListener(BM_EVENTS.workspacesChanged, loadWorkspaces);
    return () => {
      window.removeEventListener(BM_EVENTS.rolesChanged, loadRoles);
      window.removeEventListener(BM_EVENTS.providersChanged, loadModels);
      window.removeEventListener(BM_EVENTS.workspacesChanged, loadWorkspaces);
    };
  }, []);

  useEffect(() => {
    // ADR-0030:模式以服务端为准——进入/切换会话时拉取;会话刚创建时
    // (sessionsUpdated)若本地有暂存意向则上报一次后清除,防跨会话串味
    const syncFromServer = () => {
      const sid = storage.get(STORAGE_KEYS.SESSION);
      if (!sid) return;
      const pending = storage.get(STORAGE_KEYS.PERMISSION_MODE) as PermissionMode | null;
      if (pending) {
        storage.remove(STORAGE_KEYS.PERMISSION_MODE);
        setPermMode(pending);
        api
          .sessionModeSet(sid, pending)
          .catch((e) => console.warn("权限模式暂存上报失败", e));
        return;
      }
      api
        .sessionMode(sid)
        .then((d) => {
          if (d?.permission_mode) setPermMode(d.permission_mode);
        })
        .catch((e) => console.warn("权限模式拉取失败", e));
    };
    syncFromServer();
    const offSwitched = on(BM_EVENTS.sessionSwitched, syncFromServer);
    const offUpdated = on(BM_EVENTS.sessionsUpdated, syncFromServer);
    return () => {
      offSwitched();
      offUpdated();
    };
  }, []);

  const handleRoleChange = (newRoleId: string) => {
    setActiveRole(newRoleId);
    storage.set(STORAGE_KEYS.ACTIVE_ROLE, newRoleId);
    // 切换角色时清空旧会话并重开，使新角色的 system_prompt 立即绑定
    emit(BM_EVENTS.chatNew);
  };

  return (
    <ComposerPrimitive.Root className="composer">
      <ComposerPrimitive.Input
        ref={inputRef}
        className="composer-input"
        placeholder="Message BoenMind…"
        rows={2}
        autoFocus
      />
      <div className="composer-toolbar">
        {/* 1. 角色选择图标触发器 */}
        {roles.length > 0 ? (
          <Select value={activeRole} onValueChange={handleRoleChange}>
            <SelectTrigger
              size="icon"
              hideArrow
              className="bg-muted/60 h-7 w-7 border text-[13px] hover:bg-muted"
              title={`当前角色: ${roles.find((r) => r.id === activeRole)?.name || "默认"} (点击切换)`}
              data-slot="role-select"
            >
              <span>🎭</span>
            </SelectTrigger>
            <SelectContent className="rounded-lg" side="top" position="popper">
              {roles.map((r) => (
                <SelectItem key={r.id} value={r.id}>
                  🎭 {r.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : null}

        {/* 模型选择图标触发器 */}
        <Select
          value={selModel || "__default__"}
          onValueChange={(v) => {
            const val = v === "__default__" ? "" : v;
            setSelModel(val);
            if (val) storage.set(STORAGE_KEYS.ACTIVE_MODEL, val);
            else storage.remove(STORAGE_KEYS.ACTIVE_MODEL);
            emit(BM_EVENTS.activeModelChanged);
          }}
        >
          <SelectTrigger
            size="icon"
            hideArrow
            className="bg-muted/60 h-7 w-7 border text-muted-foreground hover:text-foreground hover:bg-muted"
            title={`当前模型: ${selModel || model} (点击切换)`}
            data-slot="model-select"
          >
            <Cpu size={14} />
          </SelectTrigger>
          <SelectContent
            className="rounded-lg max-w-xs"
            side="top"
            position="popper"
          >
            <SelectItem
              value="__default__"
              className="text-[12px] pl-2 [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              ⚙ 默认: {model}
            </SelectItem>
            {modelGroups.map((g) => (
              <SelectGroup key={g.provider}>
                <SelectLabel className="text-foreground/60 text-[12.5px] font-semibold">
                  {g.provider}
                </SelectLabel>
                {g.models.map((m) => (
                  <SelectItem
                    key={m}
                    value={m}
                    className="pl-7 text-[12px] [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
                  >
                    {m}
                  </SelectItem>
                ))}
              </SelectGroup>
            ))}
            {modelGroups.length === 0 ? (
              <SelectItem value="__none__" disabled>
                未设置常用——去 设置→模型 勾选
              </SelectItem>
            ) : null}
          </SelectContent>
        </Select>

        {/* 4. 工作目录选择图标触发器 */}
        <Select
          value={selWorkspace || "__auto__"}
          onValueChange={(v) => {
            const val = v === "__auto__" ? "" : v;
            setSelWorkspace(val);
            if (val) storage.set(STORAGE_KEYS.ACTIVE_WORKSPACE, val);
            else storage.remove(STORAGE_KEYS.ACTIVE_WORKSPACE);
          }}
        >
          <SelectTrigger
            size="icon"
            hideArrow
            className="bg-muted/60 h-7 w-7 border text-muted-foreground hover:text-foreground hover:bg-muted"
            title={`当前工作区: ${workspaces.find((w) => w.id === selWorkspace)?.name ?? "默认 (不绑定)"} (点击切换)`}
            data-slot="workspace-select"
          >
            <FolderOpen size={14} />
          </SelectTrigger>
          <SelectContent className="rounded-lg" side="top" position="popper">
            <SelectItem
              value="__auto__"
              className="text-[12px] pl-2 [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              默认 (不绑定工作目录)
            </SelectItem>
            {workspaces.map((w) => (
              <SelectItem
                key={w.id}
                value={w.id}
                className="py-1.5 pl-2 [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
              >
                <span className="flex flex-col">
                  <span className="text-[12.5px] font-medium">{w.name}</span>
                  <span className="text-muted-foreground font-mono text-[11px]">
                    {w.path}
                  </span>
                </span>
              </SelectItem>
            ))}
            {workspaces.length === 0 ? (
              <SelectItem value="__none__" disabled>
                暂无工作目录——去 设置→常规 添加
              </SelectItem>
            ) : null}
          </SelectContent>
        </Select>

        {/* 5. 思考等级选择图标触发器 (第2条遗漏补全) */}
        <Select
          value={thinkingLevel}
          onValueChange={(v) => {
            const val = v as ThinkingLevel;
            setThinkingLevel(val);
            storage.set(STORAGE_KEYS.THINKING_LEVEL, val);
          }}
        >
          <SelectTrigger
            size="icon"
            hideArrow
            className="bg-muted/60 h-7 w-7 border text-muted-foreground hover:text-foreground hover:bg-muted"
            title={`思考等级: ${{ off: "关闭", low: "轻度", medium: "中度", high: "深度" }[thinkingLevel]} (点击切换)`}
            data-slot="thinking-select"
          >
            <Brain
              size={14}
              className={
                thinkingLevel === "high"
                  ? "text-purple-500"
                  : thinkingLevel === "medium"
                  ? "text-blue-500"
                  : thinkingLevel === "low"
                  ? "text-emerald-500"
                  : "text-muted-foreground opacity-50"
              }
            />
          </SelectTrigger>
          <SelectContent className="w-48 rounded-lg p-1" side="top" position="popper">
            <SelectItem value="off" className="text-[12px]">
              <div className="flex flex-col">
                <span className="font-medium text-foreground">关闭思考</span>
                <span className="text-muted-foreground text-[10.5px]">仅快速流式直出回复</span>
              </div>
            </SelectItem>
            <SelectItem value="low" className="text-[12px]">
              <div className="flex flex-col">
                <span className="font-medium text-emerald-600">轻度思考 (Low)</span>
                <span className="text-muted-foreground text-[10.5px]">针对简单问答进行轻量推演</span>
              </div>
            </SelectItem>
            <SelectItem value="medium" className="text-[12px]">
              <div className="flex flex-col">
                <span className="font-medium text-blue-600">中度思考 (Medium)</span>
                <span className="text-muted-foreground text-[10.5px]">平衡的思考过程与输出速度</span>
              </div>
            </SelectItem>
            <SelectItem value="high" className="text-[12px]">
              <div className="flex flex-col">
                <span className="font-medium text-purple-600">深度思考 (High)</span>
                <span className="text-muted-foreground text-[10.5px]">充分展示详细推理步骤与细节</span>
              </div>
            </SelectItem>
          </SelectContent>
        </Select>

        {/* 6. 权限模式选择图标触发器 */}
        <Select
          value={permMode}
          onValueChange={(v) => {
            const val = v as PermissionMode;
            setPermMode(val);
            const sid = storage.get(STORAGE_KEYS.SESSION);
            if (!sid) {
              // 会话未创建:暂存意向,首条消息创建会话后自动上报服务端
              storage.set(STORAGE_KEYS.PERMISSION_MODE, val);
              return;
            }
            // ADR-0030:切换 = 发指令改服务端会话状态(落事实事件并持久);
            // 失败仅留告警,下次进入会话时以服务端值为准
            api
              .sessionModeSet(sid, val)
              .catch((e) => console.warn("权限模式切换上报失败", e));
          }}
        >
          <SelectTrigger
            size="icon"
            hideArrow
            className="bg-muted/60 h-7 w-7 border text-muted-foreground hover:text-foreground hover:bg-muted"
            title={`权限模式: ${{ ask: "变更前确认", plan: "计划模式", yolo: "完全访问(免弹窗)" }[permMode]} (点击切换)`}
            data-slot="permission-select"
          >
            {permMode === "yolo" ? (
              <Zap size={14} className="text-[var(--state-warn-fg)]" />
            ) : permMode === "plan" ? (
              <ListOrdered size={14} className="text-blue-500" />
            ) : (
              <ShieldCheck size={14} className="text-[var(--state-success-fg)]" />
            )}
          </SelectTrigger>
          <SelectContent className="w-60 rounded-lg p-1" side="top" position="popper">
            <SelectItem
              value="ask"
              className="py-2 pl-2 text-[12px] [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              <div className="flex flex-col pl-4 gap-0.5">
                <div className="flex items-center gap-1.5 font-medium text-foreground">
                  <ShieldCheck className="size-3.5 text-[var(--state-success-fg)]" />
                  <span>变更前确认</span>
                </div>
                <span className="text-muted-foreground text-[11px] leading-tight">
                  修改文件与执行命令前先问我
                </span>
              </div>
            </SelectItem>

            <SelectItem
              value="plan"
              className="py-2 pl-2 text-[12px] [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              <div className="flex flex-col pl-4 gap-0.5">
                <div className="flex items-center gap-1.5 font-medium text-foreground">
                  <ListOrdered className="size-3.5 text-blue-500" />
                  <span>计划模式</span>
                </div>
                <span className="text-muted-foreground text-[11px] leading-tight">
                  编辑与改动前先给出执行计划
                </span>
              </div>
            </SelectItem>

            <SelectItem
              value="yolo"
              className="py-2 pl-2 text-[12px] [&_[data-slot=select-item-indicator]]:left-2 [&_[data-slot=select-item-indicator]]:right-auto"
            >
              <div className="flex flex-col pl-4 gap-0.5">
                <div className="flex items-center gap-1.5 font-medium text-foreground">
                  <Zap className="size-3.5 text-[var(--state-warn-fg)]" />
                  <span>完全访问</span>
                </div>
                <span className="text-muted-foreground text-[11px] leading-tight">
                  全自动放行执行，不弹确认抽屉
                </span>
              </div>
            </SelectItem>
          </SelectContent>
        </Select>
        <span className="composer-spacer" />
        {isRunning ? (
          <ComposerPrimitive.Cancel className="send-btn stop" title="停止生成">
            <Square size={14} fill="currentColor" />
          </ComposerPrimitive.Cancel>
        ) : (
          <ComposerPrimitive.Send className="send-btn" title="发送">
            <Send size={15} />
          </ComposerPrimitive.Send>
        )}
      </div>
    </ComposerPrimitive.Root>
  );
});
