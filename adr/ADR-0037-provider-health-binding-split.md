# ADR-0037: Provider 健康与 Binding 状态的分工收口

- 状态: Accepted（2026-09-11，issue #59 残余-2 裁决）
- 日期: 2026-09-11
- 关联: ADR-0001（binding_epoch 为授权-执行-审计一致性根基）、ADR-0032（binding 代际连续性；本项为其显式排除的残余）、issue #59

## 背景

评审坐实两个状态面长期并存且互不驱动，被读作「双状态机」：

- **`BindingStatus`**（`registry.rs`，随 capabilities 行持久）：`Active`/`Draining`/`Unavailable`。生产路径中 `mark_unavailable`/`mark_recovered` **零调用**（仅测试），`Draining` **从未被赋值**，`is_available` 不进 dispatch（唯一消费方 `issue_lease`/`admit_lease` 本身生产零调用）。卸载走 `unregister` 物理摘除，不与在途协同。
- **`World.provider_health`**（`runtime.rs`，进程内）：`healthy`/`unavailable` + `fail_streak`/`reconnect_attempts`/`cooldown_until`，按 **provider** 记，是异步 dispatch 门与模型冷却门的真实输入（`turn/capability.rs` / `turn/spawn.rs`）。

两者**粒度与关注点都不同**：`BindingStatus` 按 capability 记「注册-切换-下线」的**生命周期与代际**；`provider_health` 按 provider 记「失败计数-重连-冷却」的**运行期健康**。把它们合并会混淆两件事。

## 决策

1. **分工显式化，不合并**。`BindingStatus` = 生命周期/代际状态（持久）；`provider_health` = 运行期健康（进程内）。二者非同一状态机的两份，撤销「双状态机」表述。
2. **`Draining` 获得真实语义：卸载先进排空**。`CapabilityRegistry` 增 `begin_drain`（`Active`→`Draining`，拒绝新 dispatch）与 `finish_drain`（排空后摘除）。卸载路径：无在途 → 直接摘除（现状）；有在途 → 置 `Draining`、拒绝新调用、待该能力在途调用全部落定后再摘除。在途调用的授权-执行-审计归由既有凭证 epoch 保全（ADR-0001 条件 2）。
3. **dispatch 统一查 binding 状态**（不再只有未使用的 lease API 查）。执行分道前，binding 非 `Active` → 快速失败 `ProviderUnavailable`；与 provider_health 门并存（生命周期门 + 健康门各司其职）。
4. **`restore_binding` 恢复持久状态而非硬编码 `Active`**。重启后 `unavailable` 墓碑不再被误恢复为 `Active`（ADR-0032 墓碑语义的一致性补全）。
5. **`mark_unavailable`/`mark_recovered` 归位到生命周期语义**：由 `finish_drain` 与实例切换路径使用（新实例 = epoch+1，本就是 `mark_recovered` 语义）；**运行期瞬时故障不驱动 binding**（那是 provider_health 的职责），避免连接抖动引发 epoch churn。若未来出现「provider 进程消失需翻转 dispatch 生命周期门」的真需求，再评估把 provider 级健康上提为 binding 级不可用。

## 后果

- 「谁控制 dispatch、谁记运行期健康」单处可答：生命周期门读 `BindingStatus`，健康门读 `provider_health`。
- 热重载/卸载不再在在途调用中途摘除路由；`Draining` 由死枚举变可达状态，有守护测试。
- `restore_binding` 的状态保真使重启后 `discover()` 的 `status` 与实际一致（与 ADR-0032 墓碑化配套）。
- 守护测试：`Active→Draining→摘除` 迁移、在途排空后摘除、非 Active 拒绝 dispatch、重启恢复 `unavailable` 不回升。
- `decisions.md` 增条：生命周期门（BindingStatus）与健康门（provider_health）分工，勿合并。
