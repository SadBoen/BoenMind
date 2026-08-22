import { useStore } from "../store";
import { CatalogTable } from "./CatalogTable";

export default function PluginSection() {
  const { state } = useStore();
  return <CatalogTable kind="plugin" items={state.plugins} emptyLabel="没有匹配的 SKILL/插件" />;
}
