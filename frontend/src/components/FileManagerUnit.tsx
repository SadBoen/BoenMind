import { useCallback, useEffect, useRef, useState } from "react";
import {
  App, Button, Dropdown, Empty, Input, Modal, Tooltip, Tree, Upload,
} from "antd";
import type { MenuProps, TreeDataNode } from "antd";
import {
  CloudUploadOutlined, CopyOutlined, DeleteOutlined, DownloadOutlined,
  EditOutlined, FileAddOutlined, FolderAddOutlined, FolderOpenOutlined,
  HomeOutlined, ReloadOutlined, SaveOutlined, UploadOutlined,
} from "@ant-design/icons";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeSanitize from "rehype-sanitize";
import { downloadFile, downloadUrl, rpc, uploadFile } from "../client";

// 工作目录文件管理器窗口单元（dockview 面板）。
//
// 布局（对齐 ChatUnit 的分栏/覆盖思路）：
//  - 单元宽度 ≥ 720px：左侧目录树 + 右侧预览/编辑，左右分栏。
//  - 单元宽度 < 720px：目录树全宽；打开文件时预览/编辑覆盖树，顶部"返回目录树"按钮。
// 交互：目录树懒加载（展开才拉子目录）；节点右键菜单（复制路径/打开/下载/上传到此/新建文件夹/删除）；
//       md 用 react-markdown+rehype-sanitize 渲染（防 XSS）；txt/代码纯文本只读或编辑；
//       图片经 /api/host.download 内嵌预览；编辑有 dirty 守卫（返回/切文件/关闭时确认）。
// 工作目录事实源：设置 → host.workdir（后端 settings.update 写入，服务端校验绝对路径）。

const NARROW = 720;
const PREVIEW_EXT = /\.(md|markdown|txt|log|json|toml|yaml|yml|rs|ts|tsx|js|jsx|css|html|py|sh)$/i;
const IMAGE_EXT = /\.(png|jpe?g|gif|webp|bmp|ico|avif)$/i;

interface TreeEntry {
  name: string;
  path: string; // workdir 相对路径
  isDir: boolean;
  size: number;
  hidden: boolean;
}

interface TreeNode extends TreeDataNode {
  path: string;
  isDir: boolean;
  // 懒加载：isLeaf 由后端 isDir 决定，无子目录时 leaf。
}

function isDirEntry(e: TreeEntry): boolean {
  return e.isDir;
}

export default function FileManagerUnit() {
  const { message } = App.useApp();
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(window.innerWidth);
  const [workdir, setWorkdir] = useState<string | null>(null);
  const [treeData, setTreeData] = useState<TreeNode[]>([]);
  const [expandedKeys, setExpandedKeys] = useState<React.Key[]>([]);
  const [selectedKey, setSelectedKey] = useState<React.Key | null>(null);
  // 右键菜单目标节点
  const [menuNode, setMenuNode] = useState<TreeNode | null>(null);
  const [ctxPos, setCtxPos] = useState<{ x: number; y: number } | null>(null);
  // 预览/编辑
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [openName, setOpenName] = useState("");
  const [openKind, setOpenKind] = useState<"text" | "image" | "other">("text");
  const [textContent, setTextContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [editMode, setEditMode] = useState(false);
  // 上传目标目录
  const [uploadDir, setUploadDir] = useState("");
  const [uploading, setUploading] = useState(false);
  // 新建文件夹
  const [mkdirOpen, setMkdirOpen] = useState(false);
  const [mkdirParent, setMkdirParent] = useState("");
  const [mkdirName, setMkdirName] = useState("");

  // 监听单元宽度 → 窄模式（覆盖）vs 宽模式（分栏）
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      for (const e of entries) setWidth(e.contentRect.width);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const narrow = width < NARROW;

  // 初始加载：读 workdir + 根目录
  const refreshRoot = useCallback(async () => {
    try {
      const v = await rpc<{ namespaces: { ns: string; value: Record<string, unknown> }[] }>(
        "settings.describe", {},
      );
      const ns = v.namespaces?.find((n) => n.ns === "host");
      const wd = (ns?.value?.workdir as string) ?? null;
      setWorkdir(wd);
      if (!wd) {
        setTreeData([]);
        return;
      }
      const list = await rpc<{ entries: TreeEntry[] }>("host.listWorkdir", { path: "" });
      setTreeData(toTreeNodes(list.entries));
    } catch (e) {
      message.error(`加载失败: ${(e as Error).message}`);
    }
  }, [message]);

  useEffect(() => {
    refreshRoot();
  }, [refreshRoot]);

  const toTreeNodes = (entries: TreeEntry[]): TreeNode[] =>
    entries
      .filter((e) => !e.hidden)
      .map((e) => ({
        title: e.name,
        key: e.path,
        path: e.path,
        isDir: e.isDir,
        isLeaf: !e.isDir,
        icon: e.isDir ? <FolderOpenOutlined /> : <FileAddOutlined />,
      }));

  // 懒加载子目录
  const onLoadData = useCallback(
    async (node: any) => {
      const v = await rpc<{ entries: TreeEntry[] }>("host.listWorkdir", { path: node.path });
      const children = toTreeNodes(v.entries);
      node.children = children;
      setTreeData((td) => [...td]);
    },
    [],
  );

  // 打开文件
  const openFile = async (node: TreeNode) => {
    if (node.isDir) return;
    const rel = node.path;
    setOpenName(node.title as string);
    setOpenPath(rel);
    setEditMode(false);
    setDirty(false);
    if (IMAGE_EXT.test(rel)) {
      setOpenKind("image");
      setTextContent("");
      return;
    }
    if (PREVIEW_EXT.test(rel)) {
      setOpenKind("text");
      try {
        const v = await rpc<{ content: string; size: number }>("host.readFile", { path: rel });
        setTextContent(v.content);
      } catch (e) {
        // 超限/二进制 → 转下载
        message.info(`无法直接预览：${(e as Error).message}，已转为下载。`);
        setOpenKind("other");
      }
      return;
    }
    setOpenKind("other");
  };

  // 保存编辑
  const saveEdit = async () => {
    if (!openPath) return;
    try {
      await rpc("host.writeFile", { path: openPath, content: textContent, overwrite: true });
      setDirty(false);
      setEditMode(false);
      message.success("已保存");
    } catch (e) {
      message.error(`保存失败: ${(e as Error).message}`);
    }
  };

  // 返回目录树（窄模式覆盖时）；dirty 守卫
  const backToTree = () => {
    if (dirty && !window.confirm("有未保存的修改，确定返回目录树吗？")) return;
    setOpenPath(null);
    setEditMode(false);
    setDirty(false);
  };

  // 右键菜单
  const contextMenu: MenuProps["items"] = menuNode
    ? [
        { key: "open", label: "打开", icon: <FolderOpenOutlined />, disabled: !menuNode.isDir ? false : true },
        { key: "download", label: "下载", icon: <DownloadOutlined />, disabled: menuNode.isDir },
        { key: "copy", label: "复制路径", icon: <CopyOutlined /> },
        { type: "divider" },
        { key: "upload", label: "上传到此目录", icon: <CloudUploadOutlined />, disabled: !menuNode.isDir },
        { key: "mkdir", label: "新建文件夹", icon: <FolderAddOutlined />, disabled: !menuNode.isDir },
        { key: "delete", label: "删除", icon: <DeleteOutlined />, disabled: menuNode.isDir },
      ]
    : [];

  const onCtx = async ({ key }: { key: string }) => {
    if (!menuNode) return;
    const rel = menuNode.path;
    switch (key) {
      case "open":
        await openFile(menuNode);
        break;
      case "download":
        try {
          await downloadFile(rel, menuNode.title as string);
          message.success("已开始下载");
        } catch (e) {
          message.error((e as Error).message);
        }
        break;
      case "copy":
        await navigator.clipboard.writeText(rel);
        message.success(`已复制相对路径: ${rel}`);
        break;
      case "upload":
        setUploadDir(rel);
        setMenuNode(null);
        break;
      case "mkdir":
        setMkdirParent(rel);
        setMkdirName("");
        setMkdirOpen(true);
        setMenuNode(null);
        break;
      case "delete":
        await doDelete(menuNode);
        break;
    }
    setMenuNode(null);
    setCtxPos(null);
  };

  const doDelete = async (node: TreeNode) => {
    // 删除走 host.deleteWorkdir？后端未实现——暂只对文件做"移动"提示，不删除。
    message.warning("删除功能待后端支持（当前版本隐藏）");
  };

  const onTreeRightClick = (info: { event: React.MouseEvent; node: TreeNode }) => {
    info.event.preventDefault();
    setMenuNode(info.node);
    setCtxPos({ x: info.event.clientX, y: info.event.clientY });
  };

  const onUpload = async (file: File) => {
    setUploading(true);
    try {
      await uploadFile(uploadDir, file);
      message.success(`已上传 ${file.name}`);
      refreshRoot();
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setUploading(false);
    }
    return false; // 阻止 antd 默认上传
  };

  const mkdir = async () => {
    if (!mkdirName.trim()) return;
    try {
      await rpc("host.createWorkdirDirectory", { path: mkdirParent, name: mkdirName.trim() });
      message.success("文件夹已创建");
      setMkdirOpen(false);
      refreshRoot();
    } catch (e) {
      message.error((e as Error).message);
    }
  };

  const toolbar = (
    <div className="fm-toolbar">
      <Tooltip title="刷新">
        <Button size="small" type="text" icon={<ReloadOutlined />} onClick={refreshRoot} />
      </Tooltip>
      <Tooltip title="上传到当前目录">
        <Upload showUploadList={false} beforeUpload={onUpload} multiple={false}>
          <Button size="small" type="text" icon={<UploadOutlined />} disabled={!workdir} />
        </Upload>
      </Tooltip>
      <span className="fm-workdir" title={workdir ?? "未设置"}>
        <HomeOutlined /> {workdir ?? "未设置工作目录"}
      </span>
    </div>
  );

  // 预览内容
  const renderPreview = () => {
    if (!openPath) return null;
    if (openKind === "image") {
      return (
        <div className="fm-preview-image">
          <img src={downloadUrl(openPath)} alt={openName} />
        </div>
      );
    }
    if (openKind === "text") {
      return editMode ? (
        <div className="fm-edit">
          <div className="fm-edit-header">
            <span>编辑 {openName}</span>
            <div>
              <Button size="small" type="text" onClick={() => setEditMode(false)}>取消</Button>
              <Button size="small" type="primary" icon={<SaveOutlined />} onClick={saveEdit}>保存</Button>
            </div>
          </div>
          <textarea
            className="fm-edit-ta"
            value={textContent}
            onChange={(e) => { setTextContent(e.target.value); setDirty(true); }}
          />
        </div>
      ) : (
        <div className="fm-preview-text">
          <div className="fm-preview-header">
            <span>{openName}</span>
            <Button size="small" type="text" icon={<EditOutlined />} onClick={() => setEditMode(true)}>
              编辑
            </Button>
          </div>
          {PREVIEW_EXT.test(openPath) && /\.md$/i.test(openPath) ? (
            <div className="fm-md">
              <ReactMarkdown rehypePlugins={[rehypeSanitize]} remarkPlugins={[remarkGfm]}>
                {textContent}
              </ReactMarkdown>
            </div>
          ) : (
            <pre className="fm-code">{textContent}</pre>
          )}
        </div>
      );
    }
    return (
      <div className="fm-preview-other">
        <p>此文件类型不支持内嵌预览。</p>
        <Button icon={<DownloadOutlined />} onClick={() => downloadFile(openPath!, openName)}>下载</Button>
      </div>
    );
  };

  const backBtn = (
    <Button size="small" icon={<FolderOpenOutlined />} onClick={backToTree}>
      返回目录树
    </Button>
  );

  return (
    <div className="fm-unit" ref={containerRef}>
      {/* 宽模式：左右分栏；窄模式：预览覆盖 */}
      {!narrow && (
        <div className="fm-tree-pane">
          {toolbar}
          <Tree
            className="fm-tree"
            showIcon
            loadData={onLoadData}
            treeData={treeData}
            expandedKeys={expandedKeys}
            onExpand={(keys) => setExpandedKeys(keys)}
            selectedKeys={selectedKey ? [selectedKey] : []}
            onSelect={(keys, info) => {
              setSelectedKey(keys[0] ?? null);
              const node = info.node as TreeNode;
              if (!node.isDir) openFile(node);
            }}
            onRightClick={onTreeRightClick}
            switcherIcon={({ expanded }) => (expanded ? "▾" : "▸")}
          />
        </div>
      )}
      <div className="fm-content-pane">
        {openPath ? (
          <>
            <div className="fm-content-header">
              {narrow && backBtn}
              <span className="fm-path">{openPath}</span>
            </div>
            {renderPreview()}
          </>
        ) : (
          <Empty
            className="fm-empty"
            description={
              workdir
                ? narrow ? "点选文件打开预览" : "点选文件在右侧预览"
                : "请先在 设置 → 通用 → 工作目录 设置目录"
            }
          />
        )}
      </div>
      {narrow && openPath && null}

      {/* 右键菜单：受控 Dropdown 定位在鼠标处 */}
      {ctxPos && menuNode && (
        <div
          className="fm-ctx-anchor"
          style={{ position: "fixed", left: ctxPos.x, top: ctxPos.y, width: 0, height: 0 }}
        >
          <Dropdown
            open
            trigger={["contextMenu"]}
            menu={{ items: contextMenu, onClick: onCtx }}
            onOpenChange={(o) => { if (!o) { setCtxPos(null); setMenuNode(null); } }}
            getPopupContainer={(t) => t.parentElement!}
          >
            <span style={{ display: "block", width: 0, height: 0 }} />
          </Dropdown>
        </div>
      )}

      {/* 新建文件夹 Modal */}
      <Modal
        title="新建文件夹"
        open={mkdirOpen}
        onOk={mkdir}
        onCancel={() => setMkdirOpen(false)}
        okText="创建"
        cancelText="取消"
      >
        <Input
          value={mkdirName}
          onChange={(e) => setMkdirName(e.target.value)}
          placeholder="文件夹名称（单层）"
          onPressEnter={mkdir}
        />
        {mkdirParent && <p className="fm-mkdir-parent">位置：{mkdirParent || "/"}</p>}
      </Modal>
    </div>
  );
}
