// Q1(BACKLOG:前端静态分析):ESLint 最小集——typescript-eslint 推荐规则 +
// react-hooks 推荐规则(2026-09 审计补:此前 eslint-disable 注释引用了未加载的
// react-hooks/exhaustive-deps 规则,报"rule not found";安装插件后规则真正生效)。
//
// 规则分级(2026-09 审计裁定):
// - rules-of-hooks / exhaustive-deps = error:真实缺陷面(条件调用 Hook 会崩渲染,
//   缺依赖会读陈旧闭包),严格把关;
// - set-state-in-effect / immutability = warn:React Compiler 迁移预告规则,对
//   「effect 内拉数据后 setState」「计时器归零」等 React 官方认可惯用法产生误报,
//   存量不逐处重构(避免行为风险),新代码写入时按编译器建议渐进收敛。
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";

export default tseslint.config(
  { ignores: ["dist", "node_modules", "e2e", "*.config.*"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    plugins: {
      "react-hooks": reactHooks,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-hooks/set-state-in-effect": "warn",
      "react-hooks/immutability": "warn",
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
);