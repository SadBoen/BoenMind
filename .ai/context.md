# ZCode AI 协作配置

> 本文档取代原 AGENTS.md 300+ 行，只保留核心纪律与回归清单。
> 文件地图已由 `docs/` 目录结构自解释；评审纪律已并入 `docs/architecture/decisions.md`。

## 硬纪律（违反 = 返工）

1. **合同冻结**：`boenmind-contracts/` 只增不破，删字段/改名 = Major 版本（ADR-0006）。
2. **先改模型再改文字**：代码与合同解耦，模型文件更新自动覆盖不入 git（ADR-0009）。
3. **决策写 ADR**：架构决策在 `adr/` 发新文件，熔入基线正文时标注 ADR 编号（基线 §17）。
4. **权限显式化**：能力调用必经 Broker，权限以注册合同为准（基线 §15）。
5. **里程碑 = 可运行检查点**：P0 测试套件全绿 + `validate.py` 全绿，才算完成（基线 §18）。
6. **真实进度只认 git**：主干应始终可校验，不接受"本地能跑但未提交"的进度宣称。
7. **用户可见面必须真实浏览器手测**：以页面可见内容/截图为证，不接受"代码看起来对"的交付。
8. **规范与叙事分离**：工程规格（ADR/合同/基线）不写"为什么好"的论证，辩论转录单独存档（基线 §17）。

## 环境与工具

- **Rust**: 1.98.0（`runtime/rust-toolchain.toml`；edition 2024；workspace 统一依赖 `runtime/Cargo.toml`）
- **Node.js**: v24（webapp 构建；fnm 管理）
- **Python**: 3.13（探测 `python`/`python3`/`py` 候选）
- **包管理器**: webapp 用 npm（唯一权威），勿用 pnpm（已入 `.gitignore` 防残留）

## 回归清单（提交前必跑）

```bash
# 后端
cd runtime
cargo fmt --all
cargo clippy --workspace --all-targets  # 零警告
cargo test --workspace

# 前端
cd runtime/webapp
npm run build  # tsc --noEmit + vite build

# 合同校验
python boenmind-contracts/scripts/validate.py
```

## 记忆管理（见 memory-policy.md）

- 环形队列 15 个文件上限
- 完成批次不再单独立记忆条目，改为在 `recent-work.md` 追加一行
- 坑/偏好分类合并，不新建文件
