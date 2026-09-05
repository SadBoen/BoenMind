//! 预算账本与三强制点(基线 9.7;budget.v0_1 合同)。
//!
//! ① turn_start_estimate:回合开始,剩余不足(或回合数用尽)则不发起——
//!    在创建 operation 之前执行,拒绝即返回错误,无收据(规格 §8.2)。
//! ② pre_invoke_check:模型调用前,已用量 vs 上限,超限拒绝并发布
//!    budget.exceeded(此时 operation 已在,走 waiting_model→failed)。
//! ③ post_invoke_accounting:返回后实际记账,ratio>=0.8 发布 budget.warning,
//!    超限追加 budget.exceeded。

use bm_contract::budget::round_ratio;

#[derive(Debug, Clone)]
pub struct BudgetState {
    pub max_tokens: u64,
    pub max_turns: u32,
    pub used_tokens: u64,
    pub turns_used: u32,
}

/// 强制点判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// 放行。
    Allow,
    /// 拒绝:剩余 token 不足。
    ExceededTokens,
    /// 拒绝:回合数用尽。
    ExceededTurns,
}

impl BudgetState {
    pub fn new(max_tokens: u64, max_turns: u32) -> Self {
        Self {
            max_tokens,
            max_turns,
            used_tokens: 0,
            turns_used: 0,
        }
    }

    /// 强制点①/②共用:按当前账本判断是否放行。
    pub fn check(&self, enforce_turns: bool) -> Verdict {
        if self.used_tokens >= self.max_tokens {
            return Verdict::ExceededTokens;
        }
        if enforce_turns && self.turns_used >= self.max_turns {
            return Verdict::ExceededTurns;
        }
        Verdict::Allow
    }

    pub fn remaining_tokens(&self) -> i64 {
        self.max_tokens as i64 - self.used_tokens as i64
    }

    pub fn ratio(&self) -> f64 {
        round_ratio(self.used_tokens as f64, self.max_tokens as f64)
    }

    /// 强制点③:记账。返回 (ratio, 需发 warning, 需发 exceeded)。
    pub fn account(&mut self, tokens: u64) -> (f64, bool, bool) {
        self.used_tokens = self.used_tokens.saturating_add(tokens);
        self.turns_used = self.turns_used.saturating_add(1);
        let ratio = self.ratio();
        let warn = (0.8..1.0).contains(&ratio);
        let exceeded = self.used_tokens > self.max_tokens || ratio >= 1.0;
        // 超限那一刻 warning 与 exceeded 不重复发:直接 exceeded
        (ratio, warn && !exceeded, exceeded)
    }

    /// 强制点③补充(2026-09-05 回看):失败回合也占回合配额。
    /// 上游网关对失败/超时的模型调用同样可能计费,失败不能成为绕过
    /// max_turns 的无限重试通道;token 侧网关未回执 usage,如实记 0。
    /// 返回:回合配额是否已用尽(调用方据此发 budget.exceeded)。
    pub fn account_failed_turn(&mut self) -> bool {
        self.turns_used = self.turns_used.saturating_add(1);
        self.turns_used >= self.max_turns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforcement_points_boundaries() {
        let mut b = BudgetState::new(100, 5);
        assert_eq!(b.check(true), Verdict::Allow);

        // 记账到 80%:warning
        let (ratio, warn, exceeded) = b.account(80);
        assert_eq!(ratio, 0.8);
        assert!(warn && !exceeded);
        assert_eq!(b.check(true), Verdict::Allow, "80% 时剩余 20 仍可发起");

        // 记账到 100%:恰好等于上限不算超限(used >= max 在下一回合拒绝)
        let (ratio, warn, exceeded) = b.account(20);
        assert_eq!(ratio, 1.0);
        assert!(!warn);
        assert!(exceeded, "ratio>=1.0 触发 exceeded");
        assert_eq!(b.check(true), Verdict::ExceededTokens);
    }

    #[test]
    fn turn_limit_enforced() {
        let mut b = BudgetState::new(1_000_000, 2);
        b.account(10);
        b.account(10);
        assert_eq!(b.check(true), Verdict::ExceededTurns);
        assert_eq!(b.check(false), Verdict::Allow, "不检查回合数时放行");
    }

    #[test]
    fn failed_turns_consume_turn_quota() {
        // 2026-09-05 回看:失败回合占回合配额(网关对失败调用可能同样计费),
        // token 未知如实记 0
        let mut b = BudgetState::new(1_000, 2);
        assert!(!b.account_failed_turn());
        assert_eq!(b.used_tokens, 0, "失败回合 token 记 0");
        assert_eq!(b.turns_used, 1);
        assert!(b.account_failed_turn(), "第二次失败即回合配额用尽");
        assert_eq!(b.check(true), Verdict::ExceededTurns);
        assert_eq!(b.check(false), Verdict::Allow, "token 侧不受失败记账影响");
    }
}
