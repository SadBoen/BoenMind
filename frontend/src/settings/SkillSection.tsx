import { useStore } from "../store";
import { CatalogTable } from "./CatalogTable";

export default function SkillSection() {
  const { state } = useStore();
  return <CatalogTable kind="skill" items={state.skills} emptyLabel="没有匹配的 SKILL/插件" />;
}
