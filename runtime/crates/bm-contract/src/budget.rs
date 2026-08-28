//! 预算合同镜像(budget.v0_1.schema.json)。核心维度 M1 只启用 max_tokens /
//! max_turns;结构开放,未知键必须原样保留、老版本忽略(基线 9.7)。

use crate::BmTimestamp;
use crate::ids::BmId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 开放键值:仅允许 integer/number/string/boolean(schema additionalProperties)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtraValue {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    pub max_tokens: u64,
    pub max_turns: u32,
    #[serde(flatten)]
    pub extra: BTreeMap<String, ExtraValue>,
}

/// 模型/回合返回后的实际记账(第三强制点),写 Execution Log 并更新 Agent 账本。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountingRecord {
    pub scope: BudgetScope,
    pub operation_id: BmId,
    pub used_tokens: u64,
    pub limit_tokens: u64,
    /// 四舍五入到 6 位小数(GT-A3 的 470/50000 = 0.0094 形态)。
    pub ratio: f64,
    pub at: BmTimestamp,
}

wire_str_enum!(BudgetScope {
    Agent => "agent",
});

impl BudgetScope {
    /// M1 只有 agent 作用域;M5+ 增发 task/team。
    pub const ALL: [BudgetScope; 1] = [BudgetScope::Agent];
}

/// ratio 统一经此归一,保证回放比对确定。
pub fn round_ratio(used: f64, limit: f64) -> f64 {
    if limit <= 0.0 {
        return 0.0;
    }
    (used / limit * 1e6).round() / 1e6
}
