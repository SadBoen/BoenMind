/**
 * 主布局：导航栏 + 可拖拽分栏（二级 / 主区 / 文件区）+ 状态栏。
 *
 * 布局说明：
 * - 最左侧 48px 导航栏（固定，不可拖拽）
 * - 其余三个分栏可拖拽，各有上下限（min/max 百分比）
 * - 文件区可最大化：主区折叠为 0，文件区占据自身+主区
 */
import { useCallback, useEffect } from "react";
import { Group, Panel, Separator, useGroupRef } from "react-resizable-panels";
import { NavBar } from "@/components/layout/NavBar";
import { SecondaryPanel } from "@/components/layout/SecondaryPanel";
import { MainPanel } from "@/components/layout/MainPanel";
import { FilePanel } from "@/components/files/FilePanel";
import { StatusBar } from "@/components/layout/StatusBar";
import { useAppStore } from "@/stores/app-store";

/** 分栏上下限（百分比） */
const SECONDARY_MIN = "14%";
const SECONDARY_MAX = "34%";
const MAIN_MIN = "28%";
const FILES_MIN = "16%";
const FILES_MAX = "46%";

/** 持久化布局 key（localStorage） */
const LAYOUT_KEY = "boenmind.layout.v2";

export default function App() {
  const fileMaximized = useAppStore((s) => s.fileMaximized);
  const refreshHealth = useAppStore((s) => s.refreshHealth);
  const loadConfig = useAppStore((s) => s.loadConfig);
  const loadSessions = useAppStore((s) => s.loadSessions);

  const groupRef = useGroupRef();

  // 启动加载：健康状态（轮询）+ 配置 + 会话列表
  useEffect(() => {
    void refreshHealth();
    void loadConfig();
    void loadSessions();
    const timer = setInterval(() => void refreshHealth(), 5000);
    return () => clearInterval(timer);
  }, [refreshHealth, loadConfig, loadSessions]);

  // 布局持久化：恢复上次分栏位置
  useEffect(() => {
    const saved = localStorage.getItem(LAYOUT_KEY);
    if (saved && groupRef.current) {
      try {
        groupRef.current.setLayout(JSON.parse(saved));
      } catch {
        /* ignore */
      }
    }
  }, []);

  const onLayoutChanged = useCallback((layout: Record<string, number>) => {
    // 主区折叠时（最大化中）不覆盖保存的布局
    if ((layout.main ?? 0) > 0) {
      localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout));
    }
  }, []);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
      <div className="flex min-h-0 flex-1">
        <NavBar />
        <Group
          orientation="horizontal"
          key={fileMaximized ? "files-maximized" : "normal"}
          groupRef={groupRef}
          onLayoutChanged={onLayoutChanged}
          className="min-w-0 flex-1"
        >
          <Panel id="secondary" defaultSize="20%" minSize={SECONDARY_MIN} maxSize={SECONDARY_MAX} className="min-w-0">
            <SecondaryPanel />
          </Panel>
          <Divider />
          {!fileMaximized && (
            <>
              <Panel
                id="main"
                defaultSize="54%"
                minSize={MAIN_MIN}
                className="min-w-0"
              >
                <MainPanel />
              </Panel>
              <Divider />
            </>
          )}
          <Panel
            id="files"
            defaultSize={fileMaximized ? "80%" : "26%"}
            minSize={fileMaximized ? "50%" : FILES_MIN}
            maxSize={fileMaximized ? "100%" : FILES_MAX}
            className="min-w-0"
          >
            <FilePanel />
          </Panel>
        </Group>
      </div>
      <StatusBar />
    </div>
  );
}

/** 分栏分隔线：细线 + 悬停高亮 + 可拖动（v4 库以 data-separator=active 标记拖动中） */
function Divider() {
  return (
    <Separator className="group relative w-px bg-border transition-colors hover:bg-primary/60 data-[separator=active]:bg-primary/60">
      {/* 悬停/拖动时扩大热区，便于抓取 */}
      <span className="absolute inset-y-0 -left-1.5 -right-1.5 hidden group-hover:block group-data-[separator=active]:block" />
    </Separator>
  );
}
