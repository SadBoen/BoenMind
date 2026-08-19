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
import SettingsModal from "./components/SettingsModal";
import Login from "./components/Login";
import HeaderActions from "./components/HeaderActions";
import { AuthRequiredError, getToken, rpc, setToken } from "./client";
import { getPresetId, PRESETS, setPresetId, useThemeSync, type PresetId } from "./theme";

export type AppView = "chat" | "coding";

export default function App() {
  const [authed, setAuthed] = useState<boolean | null>(null);
  const [view, setView] = useState<AppView>("chat");
  const [settingsOpen, setSettingsOpen] = useState(false);
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

  // dockview 布局：聊天单元 + 文件管理器 + 空白面板（供拖拽停靠测试）。
  const onReady = (event: DockviewReadyEvent) => {
    const api = event.api;
    const chat = api.addPanel({ id: "chat", component: "chat", title: "聊天" });
    api.addPanel({
      id: "files", component: "files", title: "文件",
      position: { referencePanel: chat, direction: "right" },
    });
    api.addPanel({
      id: "blank-1", component: "blank", title: "空白 1",
      position: { referencePanel: chat, direction: "below" },
    });
    api.addPanel({
      id: "blank-2", component: "blank", title: "空白 2",
      position: { referencePanel: chat, direction: "below" },
    });
  };

  // VS Code 连通风标题栏（悬浮标题栏样式见 styles.css，固定启用）
  const customTheme = {
    ...themeAbyss,
    className: `${themeAbyss.className} dv-theme-b`,
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
              onClick={({ key }) => {
                if (key === "settings") setSettingsOpen(true);
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
            {view === "chat" ? (
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
        {settingsOpen && (
          <SettingsModal
            preset={preset}
            onPresetChange={changePreset}
            onClose={() => setSettingsOpen(false)}
          />
        )}
      </AntdApp>
    </ConfigProvider>
  );
}
