import { useEffect, useState } from "react";
import { App as AntdApp, ConfigProvider, Menu } from "antd";
import {
  CodeOutlined,
  LogoutOutlined,
  MessageOutlined,
  SettingOutlined,
} from "@ant-design/icons";
import { DockviewReact, DockviewReadyEvent, themeAbyss } from "dockview-react";
import "dockview/dist/styles/dockview.css";
import ChatUnit from "./components/ChatUnit";
import CodingApp from "./components/CodingApp";
import FileManagerUnit from "./components/FileManagerUnit";
import SettingsPage from "./components/SettingsPage";
import Login from "./components/Login";
import HeaderActions from "./components/HeaderActions";
import StatusBar from "./components/StatusBar";
import { AuthRequiredError, getToken, rpc, setToken } from "./client";
import { getPresetId, PRESETS, setPresetId, useThemeSync, type PresetId } from "./theme";

export type AppView = "chat" | "coding" | "settings";

export default function App() {
  const [authed, setAuthed] = useState<boolean | null>(null);
  const [view, setView] = useState<AppView>("chat");
  const [preset, setPreset] = useState<PresetId>(getPresetId());

  // 启动同步主题（风格档/背景/字号 → CSS 变量 + body 背景）
  useThemeSync();

  // 启动探测：auth.status → 已认证直接进；未认证进登录页。
  // 后端未开 --auth 时 auth.status 返回 auth-not-available（非 401）→ 视为无需登录直接进。
  useEffect(() => {
    (async () => {
      try {
        const v = await rpc<{ authenticated: boolean }>("auth.status", {});
        setAuthed(v.authenticated === true);
      } catch (e) {
        if (e instanceof AuthRequiredError) setAuthed(false);
        else if ((e as Error).message?.includes("auth-not-available")) setAuthed(true);
        else setAuthed(null);
      }
    })();
  }, []);

  const onAuthed = (token: string) => {
    setToken(token);
    setAuthed(true);
  };

  const logout = async () => {
    try {
      const t = getToken();
      if (t) await rpc("auth.logout", {});
    } catch { /* ignore */ }
    setToken(null);
    setAuthed(false);
  };

  // dockview 布局：聊天单元（左）+ 文件管理器（右），默认两栏，无占位空白面板。
  const onReady = (event: DockviewReadyEvent) => {
    const api = event.api;
    const chat = api.addPanel({ id: "chat", component: "chat", title: "聊天" });
    api.addPanel({
      id: "files", component: "files", title: "文件",
      position: { referencePanel: chat, direction: "right" },
    });
  };

  // VS Code 连通风标题栏（悬浮标题栏样式见 styles.css，固定启用）。
  // tab 背景必须显式覆盖 dockview 的 activegroup/inactivegroup 变量，
  // 否则堆叠时非激活 tab 落回 abyss 默认深色（黑底）。
  const customTheme = {
    ...themeAbyss,
    className: `${themeAbyss.className} dv-theme-b`,
    activegroupVisiblePanelTabBackgroundColor: "var(--bm-bg-3)",
    activegroupVisiblePanelTabColor: "var(--bm-fg)",
    activegroupHiddenPanelTabBackgroundColor: "var(--bm-bg-2)",
    activegroupHiddenPanelTabColor: "var(--bm-fg-dim)",
    inactivegroupVisiblePanelTabBackgroundColor: "var(--bm-bg-2)",
    inactivegroupVisiblePanelTabColor: "var(--bm-fg-dim)",
    inactivegroupHiddenPanelTabBackgroundColor: "var(--bm-bg)",
    inactivegroupHiddenPanelTabColor: "var(--bm-fg-dim)",
  };

  const theme = PRESETS[preset].antd;

  const changePreset = (id: PresetId) => {
    setPreset(id);
    setPresetId(id);
  };

  if (authed === null) {
    return <div className="boot-screen">载入中…</div>;
  }
  if (authed === false) {
    return (
      <ConfigProvider prefixCls="bm" theme={theme}>
        <AntdApp>
          <Login onAuthed={onAuthed} />
        </AntdApp>
      </ConfigProvider>
    );
  }

  return (
    <ConfigProvider prefixCls="bm" theme={theme}>
      <AntdApp>
        <div className="app-shell">
          {/* 主区（导航栏 + 应用内容） */}
          <div className="app-body">
            {/* 左侧图标导航栏（antd Menu 纯图标；设置/退出固定底部） */}
            <div className="bm-sider">
              <Menu
                mode="inline"
                selectedKeys={[view]}
                onClick={({ key }) => setView(key as AppView)}
                items={[
                  { key: "chat", icon: <MessageOutlined />, label: "聊天" },
                  { key: "coding", icon: <CodeOutlined />, label: "编程" },
                ]}
              />
              <div className="bm-sider-spacer" />
              <Menu
                mode="inline"
                selectedKeys={view === "settings" ? ["settings"] : []}
                onClick={({ key }) => {
                  if (key === "settings") setView("settings");
                  else logout();
                }}
                items={[
                  { key: "settings", icon: <SettingOutlined />, label: "设置" },
                  { key: "logout", icon: <LogoutOutlined />, label: "退出登录", danger: true },
                ]}
              />
            </div>

            {/* 主区：dockview 作为布局根（antd 控件只进 panel 内部） */}
            <div className="app-main">
              {view === "settings" ? (
                <SettingsPage
                  preset={preset}
                  onPresetChange={changePreset}
                  onClose={() => setView("chat")}
                />
              ) : view === "chat" ? (
                <DockviewReact
                  theme={customTheme}
                  className="dv-theme-b"
                  singleTabMode="fullwidth"
                  onReady={onReady}
                  rightHeaderActionsComponent={HeaderActions}
                  components={{
                    chat: () => <ChatUnit />,
                    files: () => <FileManagerUnit />,
                    blank: () => <div className="pane-blank" />,
                  }}
                />
              ) : (
                <CodingApp />
              )}
            </div>
          </div>

          {/* 底部状态栏：与 App 无关的外壳层（层级同导航栏），满铺底部 */}
          <StatusBar />
        </div>
      </AntdApp>
    </ConfigProvider>
  );
}
