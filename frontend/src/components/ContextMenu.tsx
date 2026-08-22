import { useEffect, type ReactNode } from "react";
import { useStore } from "../store";
import { IconDownload, IconFolder, IconPath, IconRename, IconTrash, IconUpload } from "../lib/icons";
import { toast } from "../lib/toast";
import { findNode } from "../lib/files";

export function ContextMenu() {
  const { state, dispatch } = useStore();
  const menu = state.contextMenu;

  useEffect(() => {
    if (!menu) return;
    const close = () => dispatch({ type: "set-context-menu", menu: null });
    window.addEventListener("click", close);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [menu, dispatch]);

  if (!menu) return null;
  const node = findNode(state.files, menu.filePath);
  const isDir = node?.kind === "dir" || menu.filePath === "";

  const item = (label: string, icon: ReactNode, fn: () => void, danger = false) => (
    <button
      type="button"
      className={`menu-item${danger ? " is-danger" : ""}`}
      onClick={(e) => {
        e.stopPropagation();
        dispatch({ type: "set-context-menu", menu: null });
        fn();
      }}
    >
      {icon}
      {label}
    </button>
  );

  return (
    <div className="ctx-menu" style={{ left: menu.x, top: menu.y, position: "fixed" }} role="menu">
      {item("新建文件夹", <IconFolder />, () => {
        const name = window.prompt("文件夹名称", "新建文件夹");
        if (name) {
          dispatch({ type: "mkdir", name });
          toast.success("已新建文件夹");
        }
      })}
      {item("上传", <IconUpload />, () => {
        const input = document.createElement("input");
        input.type = "file";
        input.onchange = async () => {
          const f = input.files?.[0];
          if (!f) return;
          const content = await f.text();
          dispatch({ type: "upload", name: f.name, content });
          toast.success(`已上传 ${f.name}`);
        };
        input.click();
      })}
      {!isDir &&
        item("下载", <IconDownload />, () => {
          const blob = new Blob([node?.content ?? ""], { type: "text/plain" });
          const a = document.createElement("a");
          a.href = URL.createObjectURL(blob);
          a.download = node?.name ?? "file";
          a.click();
          toast.success("已开始下载");
        })}
      {menu.filePath &&
        item("复制路径", <IconPath />, async () => {
          await navigator.clipboard.writeText(menu.filePath);
          toast.success("路径已复制");
        })}
      {menu.filePath &&
        item("重命名", <IconRename />, () => {
          const name = window.prompt("新名称", node?.name ?? "");
          if (name) {
            dispatch({ type: "rename-file", path: menu.filePath, name });
            toast.success("已重命名");
          }
        })}
      {menu.filePath &&
        item(
          "删除",
          <IconTrash />,
          () =>
            dispatch({
              type: "ask-confirm",
              confirm: {
                title: "确认删除",
                body: `确认删除 ${node?.name ?? menu.filePath}？`,
                confirmLabel: "删除",
                danger: true,
                onConfirm: () => {
                  dispatch({ type: "delete-file", path: menu.filePath });
                  toast.success("已删除");
                },
              },
            }),
          true,
        )}
    </div>
  );
}
