import type { FileNode } from "../types";

export function findNode(nodes: FileNode[], path: string): FileNode | null {
  if (!path) return null;
  for (const n of nodes) {
    if (n.path === path) return n;
    if (n.children) {
      const hit = findNode(n.children, path);
      if (hit) return hit;
    }
  }
  return null;
}

export function nodesAt(nodes: FileNode[], cwd: string[]): FileNode[] {
  let cur = nodes;
  for (const seg of cwd) {
    const next = cur.find((n) => n.name === seg && n.kind === "dir");
    if (!next?.children) return cur;
    cur = next.children;
  }
  return cur;
}
