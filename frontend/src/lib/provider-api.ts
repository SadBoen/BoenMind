import type { ProviderKind, ProviderShape } from "../types";
import { rpc } from "./api";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(text || `${res.status} ${res.statusText}`);
  }
  return (await res.json()) as T;
}

export async function fetchProviderPresets(): Promise<Record<string, string | null> | null> {
  try {
    const data = await request<{ presets: [string, string | null][] }>("/api/providers/presets");
    return Object.fromEntries(data.presets);
  } catch {
    return null;
  }
}

/** 后端装配的真实 provider/模型目录（llm.models RPC，剥信封后取 value.groups）。 */
export interface BackendModelGroup {
  id: string;
  name: string;
  models: Array<{ id: string; name: string }>;
}

export async function fetchBackendModels(): Promise<BackendModelGroup[] | null> {
  try {
    const res = await rpc<{ groups: BackendModelGroup[] }>("llm.models", {});
    if (!res.ok) return null;
    return res.value.groups ?? [];
  } catch {
    return null;
  }
}

export async function listProviderModels(body: {
  kind: ProviderKind;
  shape?: ProviderShape;
  base_url?: string;
  api_key?: string;
}): Promise<string[]> {
  const data = await request<{ models: string[] }>("/api/providers/list-models", {
    method: "POST",
    body: JSON.stringify(body),
  });
  return data.models;
}

export async function testProvider(body: {
  kind: ProviderKind;
  shape?: ProviderShape;
  base_url?: string;
  api_key?: string;
  model?: string;
  message?: string;
}): Promise<{ ok: boolean; detail: string }> {
  return request("/api/providers/test", {
    method: "POST",
    body: JSON.stringify(body),
  });
}
