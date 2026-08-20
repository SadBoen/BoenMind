import { useEffect, useState, type ReactNode } from "react";
import {
  Alert,
  App,
  Button,
  Card,
  ColorPicker,
  Form,
  Input,
  Menu,
  Segmented,
  Select,
  Slider,
  Space,
  Switch,
  Tag,
  Typography,
  Upload,
} from "antd";
import {
  ApiOutlined,
  BgColorsOutlined,
  CheckOutlined,
  CloudUploadOutlined,
  PlusOutlined,
  ReloadOutlined,
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

// 设置中心（全页侧栏视图）：VS Code/Cursor 事实标准——导航栏进入设置应用。
// 分区：通用 / 模型与API / 外观 / 账号与数据 / 高级。
// 外观区：风格档（4 档）+ 背景替换 + 强调色 + 字号，全部 localStorage + 即时生效。
// 全页形态相对弹窗：全程可见导航（分区可跳转、状态不因关闭重置），
// 内容随分区渲染，改动即时生效可并排观察主界面效果。

type SectionId = "general" | "models" | "appearance" | "account" | "advanced";

interface ProviderInfo {
  provider: string;
  displayName: string;
  settingsNs: string;
  active: boolean;
}

/** 单 provider 的运行态编辑状态：baseURL（live 写 llm.<id>）、
 *  模型发现结果、发现的模型清单、默认模型选择。 */
interface ProviderEditState {
  baseURL: string;
  baseUrlSaved: boolean;
  models: { id: string; name: string }[];
  discovered: boolean;
  discovering: boolean;
  defaultModel: string;
  keySet: boolean;
  keyDirty: boolean;
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

export default function SettingsPage({ onClose, preset, onPresetChange }: Props) {
  const { message } = App.useApp();
  const [section, setSection] = useState<SectionId>("general");
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  // 每个 provider 的编辑态（keyed by settingsNs）
  const [providerStates, setProviderStates] = useState<Record<string, ProviderEditState>>({});
  // 每个 provider 待提交的 API Key 输入值（不落 state 会闪失）
  const [keyInputs, setKeyInputs] = useState<Record<string, string>>({});

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
      .then((v) => {
        const list = v.providers ?? [];
        setProviders(list);
        // 初始化每个 provider 的编辑态：读已存 baseURL + 凭据 configured
        if (list.length > 0) {
          const nsNames = list.map((p) => p.settingsNs);
          void rpc<{ namespaces: { ns: string; value: Record<string, unknown> }[] }>(
            "settings.describe",
            {}
          ).then((sv) => {
            const states: Record<string, ProviderEditState> = {};
            for (const p of list) {
              const nsVal = sv.namespaces.find((n) => n.ns === p.settingsNs)?.value ?? {};
              states[p.settingsNs] = {
                baseURL: (nsVal.baseURL as string) || "",
                baseUrlSaved: !!(nsVal.baseURL as string),
                models: [],
                discovered: false,
                discovering: false,
                defaultModel: "",
                keySet: false,
                keyDirty: false,
              };
            }
            setProviderStates(states);
          });
          void rpc<{ credentials: Record<string, { configured: boolean; writable: boolean }> }>(
            "credentials.describe",
            { refs: list.map((p) => `${p.provider.toUpperCase()}_API_KEY`) }
          ).then((cv) => {
            setProviderStates((prev) => {
              const next = { ...prev };
              for (const p of list) {
                const st = next[p.settingsNs];
                if (st) {
                  st.keySet = cv.credentials[`${p.provider.toUpperCase()}_API_KEY`]?.configured ?? false;
                }
              }
              return next;
            });
          });
        }
      })
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

  /** 保存某 provider 的 baseURL（llm.<id> 写面，live 同步适配器，下一请求生效） */
  const saveProviderBaseUrl = async (p: ProviderInfo) => {
    const st = providerStates[p.settingsNs];
    const url = (st?.baseURL ?? "").trim();
    try {
      await rpc("settings.update", { ns: p.settingsNs, patch: url ? { baseURL: url } : {} });
      setProviderStates((prev) => ({
        ...prev,
        [p.settingsNs]: { ...prev[p.settingsNs], baseUrlSaved: !!url },
      }));
      message.success(url ? `${p.displayName} baseURL 已更新（下一请求生效）` : `${p.displayName} baseURL 已恢复默认`);
    } catch (e) {
      message.error(`保存失败: ${(e as Error).message}`);
    }
  };

  /** 设置某 provider 的 API key（credentials.set {ID}_API_KEY，热补 adapter，不落盘明文） */
  const saveProviderKey = async (p: ProviderInfo, key: string) => {
    if (!key.trim()) {
      message.warning("请输入 API Key");
      return;
    }
    try {
      await rpc("credentials.set", { ref: `${p.provider.toUpperCase()}_API_KEY`, value: key.trim() });
      setProviderStates((prev) => ({
        ...prev,
        [p.settingsNs]: { ...prev[p.settingsNs], keySet: true, keyDirty: false },
      }));
      setKeyInputs((prev) => ({ ...prev, [p.settingsNs]: "" }));
      message.success(`${p.displayName} API Key 已设置`);
    } catch (e) {
      message.error(`设置失败: ${(e as Error).message}`);
    }
  };

  /** 远程发现某 provider 的模型（llm.discoverModels，特权；失败返回 baseURL 帮助诊断） */
  const discoverModels = async (p: ProviderInfo) => {
    setProviderStates((prev) => ({
      ...prev,
      [p.settingsNs]: { ...prev[p.settingsNs], discovering: true },
    }));
    try {
      const v = await rpc<{ models: { id: string; name: string }[]; baseURL?: string }>(
        "llm.discoverModels",
        { settingsNs: p.settingsNs }
      );
      setProviderStates((prev) => ({
        ...prev,
        [p.settingsNs]: {
          ...prev[p.settingsNs],
          models: v.models ?? [],
          discovered: true,
          discovering: false,
        },
      }));
      message.success(`发现 ${(v.models ?? []).length} 个模型`);
    } catch (e) {
      setProviderStates((prev) => ({
        ...prev,
        [p.settingsNs]: { ...prev[p.settingsNs], discovering: false },
      }));
      message.error(`发现失败: ${(e as Error).message}`);
    }
  };

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
    <div className="settings-page">
      <aside className="settings-page-nav">
        <div className="settings-page-head">
          <SettingOutlined />
          <span>设置</span>
        </div>
        <Menu
          mode="inline"
          selectedKeys={[section]}
          onClick={({ key }) => setSection(key as SectionId)}
          items={SECTIONS.map((s) => ({ key: s.id, icon: s.icon, label: s.label }))}
          className="settings-nav"
        />
        <div className="settings-page-foot">
          <Button icon={<CheckOutlined />} onClick={onClose}>
            完成
          </Button>
        </div>
      </aside>
      <main className="settings-content">
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
              已装配的 provider（来自后端 llm.providers）。可 live 改 baseURL
              （下一请求生效）、热补 API Key、远程发现模型。
            </p>
            {providers.map((p) => {
              const st = providerStates[p.settingsNs] ?? {
                baseURL: "", baseUrlSaved: false, models: [], discovered: false,
                discovering: false, defaultModel: "", keySet: false, keyDirty: false,
              };
              return (
                <Card
                  key={p.settingsNs}
                  size="small"
                  className="provider-card"
                  title={
                    <span className="provider-card-title">
                      <ApiOutlined /> {p.displayName}
                      <Tag className="provider-tag" color={p.active ? "blue" : "default"}>
                        {p.active ? "已启用" : "未启用"}
                      </Tag>
                      <span className="provider-id">{p.provider}</span>
                    </span>
                  }
                >
                  <SettingRow label="Base URL" desc="API 端点（留空 = 恢复装配默认）">
                    <div className="pwd-row">
                      <Input
                        placeholder="https://api.example.com/v1"
                        value={st.baseURL}
                        onChange={(e) =>
                          setProviderStates((prev) => ({
                            ...prev,
                            [p.settingsNs]: { ...prev[p.settingsNs], baseURL: e.target.value },
                          }))
                        }
                      />
                      <Button onClick={() => saveProviderBaseUrl(p)}>保存</Button>
                    </div>
                  </SettingRow>
                  <SettingRow
                    label="API Key"
                    desc={st.keySet ? "已配置（密钥不回显，重填即覆盖）" : "未配置——请求将报 MISSING_CREDENTIAL"}
                  >
                    <div className="pwd-row">
                      <Input.Password
                        placeholder="sk-…"
                        value={keyInputs[p.settingsNs] ?? ""}
                        onChange={(e) => {
                          setKeyInputs((prev) => ({ ...prev, [p.settingsNs]: e.target.value }));
                          setProviderStates((prev) => ({
                            ...prev,
                            [p.settingsNs]: { ...prev[p.settingsNs], keyDirty: true },
                          }));
                        }}
                        onPressEnter={() => saveProviderKey(p, keyInputs[p.settingsNs] ?? "")}
                      />
                      <Button type="primary" onClick={() => saveProviderKey(p, keyInputs[p.settingsNs] ?? "")} disabled={!st.keyDirty}>
                        保存 Key
                      </Button>
                    </div>
                  </SettingRow>
                  <SettingRow label="模型发现" desc="从该 provider 拉取模型清单（llm.discoverModels）">
                    <div className="pwd-row">
                      <Button
                        icon={<ReloadOutlined />}
                        loading={st.discovering}
                        onClick={() => discoverModels(p)}
                      >
                        {st.discovered ? `重新发现（${st.models.length}）` : "发现模型"}
                      </Button>
                      {st.discovered && st.models.length > 0 && (
                        <Select
                          className="settings-control"
                          placeholder="默认模型"
                          value={st.defaultModel || undefined}
                          onChange={(m) =>
                            setProviderStates((prev) => ({
                              ...prev,
                              [p.settingsNs]: { ...prev[p.settingsNs], defaultModel: m },
                            }))
                          }
                          options={st.models.map((m) => ({ value: m.id, label: m.name }))}
                        />
                      )}
                    </div>
                  </SettingRow>
                </Card>
              );
            })}
            {providers.length === 0 && <p className="settings-hint">加载中…</p>}
            <div className="settings-hint provider-custom-hint">
              <PlusOutlined style={{ marginRight: 4 }} />
              添加自定义 provider（OpenAI 兼容端点）会写入 config.toml，需重启服务生效——见「高级」。
            </div>
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
      </main>
    </div>
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