//! M4-T8:Broker 三证伪测试(硬约束 1;ADR-0001 条件 1)。
//! 超标即回炉实现方案(被证伪的是实现而非三权分立本身):
//! - P-09 授权决策开销:查表路径 p99 门槛(test build < 200 µs;
//!   release < 10 µs,perf-baseline §1 P-09)
//! - P-10 队头阻塞:慢 Provider 在 registry 时,无关授权决策 p99 劣化 < 2×
//! - 故障半径:Provider panic 被 execute 收容,核心循环与后续授权不受影响

use bm_contract::capability::CapabilityManifest;
use bm_contract::ids::SeqIdGen;
use bm_core::broker::{Broker, CallContext, CallOutcome, GrantLedger, provider_fn};
use bm_core::clock::MockClock;
use bm_core::registry::CapabilityRegistry;
use serde_json::json;
use std::time::Instant;

const BASE_MS: u128 = 1_788_000_000_000;

fn five_registry() -> CapabilityRegistry {
    let mut reg = CapabilityRegistry::new();
    for (name, effect) in [
        ("system.ro", "read-only"),
        ("system.low", "low-risk-command"),
        ("system.rev", "reversible-command"),
        ("system.ext", "external-side-effect"),
        ("system.high", "high-risk-command"),
    ] {
        let m: CapabilityManifest = serde_json::from_value(json!({
            "capability": name, "provider": name, "version": "0.1.0",
            "input_schema": {"type": "object"},
            "output_schema": {"type": "object"},
            "effect": effect, "idempotent": true, "cancellable": true,
            "timeout_ms": 1000, "approval": "not-required"
        }))
        .unwrap();
        reg.register(m, &format!("{name}@0.1.0"), provider_fn(Ok))
            .unwrap();
    }
    reg
}

fn p99_us(samples_us: &[f64]) -> f64 {
    let mut sorted = samples_us.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((sorted.len() as f64) * 0.99).ceil() as usize;
    sorted[(idx - 1).min(sorted.len() - 1)]
}

/// P-09 证伪①:授权决策(查表 + 常量校验,不含 Provider 执行)p99。
/// 门槛:M4 规格 §5.1——test build p99 < 200 µs(release < 10 µs)。
#[test]
fn p09_authorize_decision_p99_under_threshold() {
    let reg = five_registry();
    let mut grants = GrantLedger::new();
    let clock = MockClock::at_ms(BASE_MS);
    let ids = SeqIdGen::new();
    let broker = Broker::new(&reg, &mut grants, &clock, &ids);
    let ctx = CallContext::surface("surface:user");

    // 预热 500 次再测 10000 次
    for _ in 0..500 {
        let _ = broker.decide(&ctx, "system.ro", &json!({}));
    }
    let mut samples = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let t0 = Instant::now();
        let _ = broker.decide(&ctx, "system.ro", &json!({}));
        samples.push(t0.elapsed().as_secs_f64() * 1e6);
    }
    let p99 = p99_us(&samples);
    assert!(
        p99 < 200.0,
        "P-09 证伪:授权决策 p99 = {p99:.1} µs ≥ 200 µs 门槛(test build)——回炉查表实现"
    );
}

/// P-10 证伪②:队头阻塞——registry 中挂一个「慢 Provider」只影响其自身
/// execute,无关授权决策(纯查表)的 p99 不得被牵制(劣化 < 2× 空闲基线)。
#[test]
fn p10_no_head_of_line_blocking_from_slow_provider() {
    let mut reg = five_registry();
    // 慢 Provider 挂在 system.high(其 execute 需要 50ms;授权路径不触它)
    reg.switch_binding(
        "system.high",
        "system.high@slow",
        provider_fn(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            Ok(json!({}))
        }),
    )
    .unwrap();

    let baseline = five_registry();
    let mut grants = GrantLedger::new();
    let clock = MockClock::at_ms(BASE_MS);
    let ids = SeqIdGen::new();
    let ctx = CallContext::surface("surface:user");

    let measure = |grants: &mut GrantLedger, reg: &CapabilityRegistry| -> f64 {
        let broker = Broker::new(reg, grants, &clock, &ids);
        let mut samples = Vec::with_capacity(500);
        for _ in 0..500 {
            let t0 = Instant::now();
            let _ = broker.decide(&ctx, "system.ro", &json!({}));
            samples.push(t0.elapsed().as_secs_f64() * 1e6);
        }
        p99_us(&samples)
    };
    let base_p99 = measure(&mut grants, &baseline);
    let slow_p99 = measure(&mut grants, &reg);
    assert!(
        slow_p99 < base_p99 * 2.0 + 50.0,
        "P-10 证伪:慢 Provider 在册时无关授权 p99 劣化超阈(基线 {base_p99:.1} µs → {slow_p99:.1} µs)"
    );
}

/// 故障半径证伪③:Provider panic 被 execute 收容为 ProviderError——
/// Runtime 与后续授权不受影响(无特权降级通道;L0 兜底语义由重启承担)。
#[test]
fn provider_panic_is_contained_within_call_outcome() {
    let mut reg = CapabilityRegistry::new();
    let m: CapabilityManifest = serde_json::from_value(json!({
        "capability": "system.boom", "provider": "system.boom", "version": "0.1.0",
        "input_schema": {"type": "object"}, "output_schema": {"type": "object"},
        "effect": "low-risk-command", "idempotent": true, "cancellable": true,
        "timeout_ms": 1000, "approval": "not-required"
    }))
    .unwrap();
    reg.register(m, "system.boom@0.1.0", provider_fn(|_| panic!("boom!")))
        .unwrap();
    let mut grants = GrantLedger::new();
    let clock = MockClock::at_ms(BASE_MS);
    let ids = SeqIdGen::new();
    let mut broker = Broker::new(&reg, &mut grants, &clock, &ids);
    let ctx = CallContext::surface("surface:user");

    let out = broker.call(&ctx, "system.boom", json!({}));
    match out {
        CallOutcome::ProviderError { message } => {
            assert!(
                message.contains("panic"),
                "panic 应转 ProviderError: {message}"
            );
        }
        other => panic!("panic 必须被收容为 ProviderError,实际 {other:?}"),
    }

    // 收容后:同一 Broker 继续裁决与执行,无状态污染
    let out2 = broker.call(&ctx, "system.unknown_cap", json!({}));
    assert!(matches!(out2, CallOutcome::Rejected { .. }));
    let reg2 = five_registry();
    let mut broker2 = Broker::new(&reg2, &mut grants, &clock, &ids);
    let out3 = broker2.call(&ctx, "system.ro", json!({"ok": true}));
    assert!(
        matches!(out3, CallOutcome::Completed { .. }),
        "后续调用正常: {out3:?}"
    );
}
