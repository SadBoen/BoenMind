import { useEffect, useState, type ReactNode } from "react";
import {
  Alert,
  App,
  Button,
  ColorPicker,
  Form,
  Input,
  Menu,
  Modal,
  Segmented,
  Select,
  Slider,
  Switch,
  Typography,
  Upload,
} from "antd";
import {
  BgColorsOutlined,
  CloudUploadOutlined,
  RobotOutlined,
  SettingOutlined,
  ToolOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { rpc } from "../client";
import {
  BackgroundValue,
  BACKGROUNDS,
  getAccent,
  getBackground,
  getFontSize,
  getPresetId,
  PRESETS,
  PresetId,
  setAccent,
  setBackground,
  setFontSize,
  applyPresetChange,
  applyBackground,
  applyFontSize,
} from "../theme";

// 设置中心：antd Modal + 左导航（VS Code/Cursor 事实标准）。
// 分区：通用 / 模型与API / 外观 / 账号与数据 / 高级。
// 外观区：风格档（4 档）+ 背景替换 + 强调色 + 字号，全部 localStorage + 即时生效。

type SectionId = "general" | "models" | "appearance" | "account" | "advanced";

interface ProviderInfo {
  provider: string;
  displayName: string;
  settingsNs: string;
  active: boolean;
}

interface Props {
  onClose: () => void;
  preset: PresetId;
  onPresetChange: (id: PresetId) => void;
}

const SECTIONS: { id: SectionId; label: string; icon: ReactNode }[] = [
  { id: "general", label: "通用", icon: <SettingOutlined /> },
  { id: "models", label: "模型与 API", icon: <RobotOutlined /> },
  { id: "appearance", label: "外观", icon: <BgColorsOutlined /> },
  { id: "account", label: "账号与数据", icon: <UserOutlined /> },
  { id: "advanced", label: "高级", icon: <ToolOutlined /> },
];

export default function SettingsModal({ onClose, preset, onPresetChange }: Props) {
  const { message } = App.useApp();
  const [section, setSection] = useState<SectionId>("general");
  const [providers, setProviders] = useState<ProviderInfo[]>([]);

  // 外观状态
  const [accent, setAccentState] = useState(getAccent());
  const [fontSize, setFontSizeState] = useState(getFontSize());
  const [bgId, setBgId] = useState<BackgroundValue["type"]>(
    getBackground().type === "image" ? "image" : getBackground().type
  );
  const [bgUrl, setBgUrl] = useState(
    getBackground().type === "image" ? getBackground().value ?? "" : ""
  );

  // 工作目录（通用区）
  const [workdir, setWorkdirState] = useState("");
  const [workdirSet, setWorkdirSet] = useState(false);

  // 改密
  const [currentPwd, setCurrentPwd] = useState("");
  const [newPwd, setNewPwd] = useState("");
  const [pwdMsg, setPwdMsg] = useState<string | null>(null);
  const [pwdBusy, setPwdBusy] = useState(false);

  useEffect(() => {
    rpc<{ providers: ProviderInfo[] }>("llm.providers", {})
      .then((v) => setProviders(v.providers ?? []))
      .catch(() => {});
    // 读取已存工作目录（settings host.workdir）
    rpc<{ namespaces: { ns: string; value: Record<string, unknown> }[] }>("settings.describe", {})
      .then((v) => {
        const ns = v.namespaces.find((n) => n.ns === "host");
        const wd = (ns?.value?.workdir as string) || "";
        setWorkdirState(wd);
        setWorkdirSet(!!wd);
      })
      .catch(() => {});
  }, []);

  // 保存工作目录（服务端校验绝对路径 + 存在可读）
  const saveWorkdir = async () => {
    try {
      await rpc("settings.update", { ns: "host", patch: { workdir: workdir.trim() } });
      setWorkdirSet(!!workdir.trim());
      message.success(workdir.trim() ? "工作目录已设置" : "工作目录已清空");
      // 通知文件管理器重载（若已挂载）
      window.dispatchEvent(new CustomEvent("bm-workdir-changed"));
    } catch (e) {
      message.error(`设置失败: ${(e as Error).message}`);
    }
  };

  const changePreset = (id: PresetId) => {
    onPresetChange(id);
    applyPresetChange(id, accent);
  };

  const changeAccent = (color: string) => {
    setAccentState(color);
    setAccent(color);
    applyPresetChange(preset, color);
  };

  const changeFont = (v: number) => {
    setFontSizeState(v);
    setFontSize(v);
    applyFontSize(v);
  };

  const changeBg = (type: BackgroundValue["type"]) => {
    setBgId(type);
    if (type === "gradient") {
      const bg: BackgroundValue = { type: "gradient" };
      setBackground(bg);
      applyBackground(bg);
    } else if (type === "image") {
      // 图片需 URL 或上传后才生效（保持当前值）
    } else {
      const bg: BackgroundValue = { type: "none" };
      setBackground(bg);
      applyBackground(bg);
    }
  };

  const applyBgImage = (value: string) => {
    if (!value) return;
    const bg: BackgroundValue = { type: "image", value };
    setBackground(bg);
    applyBackground(bg);
    message.success("背景已应用");
  };

  const changePwd = async () => {
    setPwdMsg(null);
    if (!currentPwd || !newPwd) {
      setPwdMsg("请填写当前密码和新密码");
      return;
    }
    setPwdBusy(true);
    try {
      await rpc("auth.changePassword", { currentPassword: currentPwd, newPassword: newPwd });
      setPwdMsg("✅ 密码已修改");
      setCurrentPwd("");
      setNewPwd("");
    } catch (e) {
      setPwdMsg(`❌ ${(e as Error).message}`);
    } finally {
      setPwdBusy(false);
    }
  };

  return (
    <Modal
      open
      title="设置"
      width={760}
      onCancel={onClose}
      footer={null}
      destroyOnClose
      className="bm-settings-modal"
    >
      <div className="settings-body">
        <Menu
          mode="inline"
          selectedKeys={[section]}
          onClick={({ key }) => setSection(key as SectionId)}
          items={SECTIONS.map((s) => ({ key: s.id, icon: s.icon, label: s.label }))}
          className="settings-nav"
        />
        <div className="settings-content">
          {section === "general" && (
            <SettingSection title="通用">
              <SettingRow
                label="工作目录"
                desc={workdirSet ? "文件管理器以此目录为根（绝对路径）" : "未设置——文件管理器需先设置工作目录"}
              >
                <div className="pwd-row">
                  <Input
                    placeholder="如 D:\\work 或 /home/user/work"
                    value={workdir}
                    onChange={(e) => setWorkdirState(e.target.value)}
                  />
                  <Button onClick={saveWorkdir}>保存</Button>
                </div>
              </SettingRow>
              <SettingRow label="界面语言" desc="界面显示语言（占位，i18n 后置）">
                <Select
                  className="settings-control"
                  defaultValue="zh-CN"
                  options={[
                    { value: "zh-CN", label: "简体中文" },
                    { value: "en", label: "English" },
                  ]}
                />
              </SettingRow>
              <SettingRow label="启动行为" desc="启动时自动恢复上次会话">
                <Switch defaultChecked />
              </SettingRow>
              <SettingRow label="遥测" desc="匿名使用统计（占位）">
                <Switch />
              </SettingRow>
            </SettingSection>
          )}

          {section === "models" && (
            <SettingSection title="模型与 API">
              <p className="settings-hint">
                Provider 列表（来自后端 llm.providers）；API Key 输入占位，保存/验证后置。
              </p>
              {providers.map((p) => (
                <SettingRow
                  key={p.provider}
                  label={p.displayName}
                  desc={`${p.provider} · ${p.active ? "已启用" : "未配置"}`}
                >
                  <Input.Password
                    className="settings-control"
                    placeholder="••••••••"
                    defaultValue={p.active ? "sk-••••1234" : ""}
                  />
                </SettingRow>
              ))}
              {providers.length === 0 && <p className="settings-hint">加载中…</p>}
            </SettingSection>
          )}

          {section === "appearance" && (
            <SettingSection title="外观">
              <SettingRow label="风格" desc="整体设计风格（Ant 蓝白红 / 卡通多彩 / 玻璃 / 暗黑）">
                <Segmented
                  value={preset}
                  onChange={(v) => changePreset(v as PresetId)}
                  options={Object.values(PRESETS).map((p) => ({ value: p.id, label: p.label }))}
                />
              </SettingRow>
              <SettingRow label="背景" desc="整体背景（渐变/本地图片/URL；动态背景预留位）">
                <Segmented
                  value={bgId}
                  onChange={(v) => changeBg(v as BackgroundValue["type"])}
                  options={BACKGROUNDS.map((b) => ({ value: b.id, label: b.label }))}
                />
              </SettingRow>
              {bgId === "image" && (
                <SettingRow label="图片背景" desc="输入图片 URL，或上传本地图片（<2MB）">
                  <div className="bg-image-row">
                    <Input
                      placeholder="https://…/bg.jpg"
                      value={bgUrl}
                      onChange={(e) => setBgUrl(e.target.value)}
                      onPressEnter={() => applyBgImage(bgUrl.trim())}
                    />
                    <Button onClick={() => applyBgImage(bgUrl.trim())}>应用</Button>
                    <Upload
                      accept="image/*"
                      showUploadList={false}
                      beforeUpload={(file) => {
                        if (file.size > 2 * 1024 * 1024) {
                          message.warning("图片过大，请使用 URL（上限 2MB）");
                          return Upload.LIST_IGNORE;
                        }
                        const reader = new FileReader();
                        reader.onload = () => {
                          const url = reader.result as string;
                          setBgUrl("");
                          applyBgImage(url);
                        };
                        reader.readAsDataURL(file);
                        return false;
                      }}
                    >
                      <Button icon={<CloudUploadOutlined />}>上传</Button>
                    </Upload>
                  </div>
                </SettingRow>
              )}
              <SettingRow label="强调色" desc="界面主色（即时生效）">
                <ColorPicker
                  value={accent}
                  onChange={(c) => changeAccent(c.toHexString())}
                  showText
                />
              </SettingRow>
              <SettingRow label="字号" desc={`${fontSize}px（界面基础字号）`}>
                <Slider
                  className="settings-control"
                  min={12}
                  max={18}
                  step={1}
                  value={fontSize}
                  onChange={(v) => changeFont(v as number)}
                />
              </SettingRow>
            </SettingSection>
          )}

          {section === "account" && (
            <SettingSection title="账号与数据">
              <SettingRow label="修改密码" desc="当前密码 + 新密码（真调 auth.changePassword）">
                <div className="pwd-row">
                  <Input.Password
                    placeholder="当前密码"
                    value={currentPwd}
                    onChange={(e) => setCurrentPwd(e.target.value)}
                  />
                  <Input.Password
                    placeholder="新密码"
                    value={newPwd}
                    onChange={(e) => setNewPwd(e.target.value)}
                  />
                  <Button type="primary" loading={pwdBusy} onClick={changePwd}>
                    修改
                  </Button>
                </div>
              </SettingRow>
              {pwdMsg && <p className="settings-hint">{pwdMsg}</p>}
              <SettingRow label="会话数据" desc="本地存储（boenmind.db + settings.json）">
                <span className="settings-static">本地</span>
              </SettingRow>
            </SettingSection>
          )}

          {section === "advanced" && (
            <SettingSection title="高级">
              <SettingRow label="重置布局" desc="dockview 布局恢复默认（占位）">
                <Button onClick={() => location.reload()}>重置</Button>
              </SettingRow>
              <SettingRow label="关于" desc="BoenMind · Rust 微内核 agent 平台">
                <span className="settings-static">v0.1.0</span>
              </SettingRow>
            </SettingSection>
          )}
        </div>
      </div>
    </Modal>
  );
}

function SettingSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="setting-section">
      <div className="setting-section-title">{title}</div>
      {children}
    </div>
  );
}

function SettingRow({ label, desc, children }: { label: string; desc?: string; children: ReactNode }) {
  return (
    <div className="setting-row">
      <div className="setting-row-text">
        <div className="setting-row-label">{label}</div>
        {desc && <div className="setting-row-desc">{desc}</div>}
      </div>
      <div className="setting-row-control">{children}</div>
    </div>
  );
}
