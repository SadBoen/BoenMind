# 插件吸收台账（2026-08-19）

> 机制（grok 评审 P0）：**源仓冻结 → 产品 crate 吸收，不双向同步**。
> dsh-rust-plugins = 上游实验/社区源；BoenMind/plugins = 产品冻结实现。
> 更新流程：源仓打新 tag → 对比 diff → 确认后复制进 BoenMind/plugins → 更新本台账。

## 已吸收（2026-08-19）

| 插件 | 源仓 commit | 源仓 tag | BoenMind 位置 | 差异 |
|---|---|---|---|---|
| plugin-llm | `a906f8c` | `absorbed-into-boenmind-2026-08-19` | `plugins/plugin-llm/` | 仅 Cargo.toml path（`../../dsh-rust-core/*` → `../../kernel/*`）；src 零差异 |
| plugin-loop | `a906f8c` | 同上 | `plugins/plugin-loop/` | 同上 |
| plugin-tools | `a906f8c` | 同上 | `plugins/plugin-tools/` | 同上 |

## 验证

- `diff -r plugins/<p>/src ../dsh-rust-plugins/<p>/src` 无输出（src 一致）。
- BoenMind `cargo test --workspace`：三插件测试全过（plugin-llm 38 / plugin-loop 9 / plugin-tools 5）。

## 更新流程（吸收新版本）

1. `cd ../dsh-rust-plugins && git log --oneline` 看上游新提交
2. `git tag -a absorbed-into-boenmind-<date> <commit>` + push
3. `diff -r plugins/<p>/src ../dsh-rust-plugins/<p>/src` 确认改动
4. 复制进 BoenMind/plugins（Cargo.toml 改 path 指 `../../kernel/`）
5. 跑 BoenMind 全量验证，更新本台账
