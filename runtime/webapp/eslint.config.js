// Q1(BACKLOG:前端静态分析):ESLint 最小集——typescript-eslint 推荐规则,
// 不带 stylistic(风格交给 fmt/约定);关键红线:no-explicit-any 关(存量巨大,
// 渐进收紧)。CI 随后续批接入,先本地 npm run lint。
import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "node_modules", "e2e", "*.config.*"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    rules: {
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },
);
