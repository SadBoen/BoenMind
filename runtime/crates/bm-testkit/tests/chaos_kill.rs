//! S4 崩溃恢复·混沌①(ADR-0004 条件 8 的 M2 适配映射,规格 §6-T4):
//! 真实进程硬杀(TerminateProcess / taskkill /F 同源)后重启:
//! - 事件日志无半写(逐行可解析);
//! - 状态位点 ≤ 日志末尾(先日志后状态写序的互为校验);
//! - 恢复后 operation 落 interrupted,会话仍 active,收据可查询(INV-6);
//! - runtime.recovered 报告 interrupted_recovered = 1。

use std::process::{Command, Stdio};
use std::time::Duration;

fn child_exe() -> &'static str {
    env!("CARGO_BIN_EXE_chaos-child")
}

#[tokio::test]
async fn t22_hard_kill_process_recovery() {
    let dir = tempfile::tempdir().expect("临时目录");
    let dir_path = dir.path().to_path_buf();

    // ① 子进程 run:建会话 + 长回合,等 marker 出现
    let mut child = Command::new(child_exe())
        .arg(&dir_path)
        .arg("run")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("拉起 chaos-child");

    let marker = dir_path.join("chaos_marker");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if marker.exists() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "30s 内未出现 marker(回合未发起),测试环境异常"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let killed_op_id = std::fs::read_to_string(&marker).expect("读 marker");

    // ② 硬杀(Windows = TerminateProcess,与 taskkill /F 同源;无优雅停机)
    child.kill().expect("硬杀子进程");
    let status = child.wait().expect("回收子进程");
    assert!(!status.success(), "被硬杀的进程不应报告成功退出");

    // ③ 父进程直接校验持久层:无半写、位点自洽
    {
        let store = bm_persist::PersistStore::open(&dir_path).expect("日志逐行可解析、位点自洽");
        let log_last = bm_persist::EventStore::last_log_seq(&store).expect("日志末尾");
        let applied = bm_persist::EventStore::last_applied_seq(&store).expect("位点");
        assert!(applied <= log_last, "先日志后状态:位点不得超前");
        assert!(log_last >= 4, "回合事件已落盘(至少 4 条),实际 {log_last}");
    }

    // ④ 子进程 verify:真实重启(内部触发启动恢复),打印恢复面 JSON
    let output = Command::new(child_exe())
        .arg(&dir_path)
        .arg("verify")
        .output()
        .expect("拉起 verify 子进程");
    assert!(
        output.status.success(),
        "verify 应成功: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let report: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("verify 输出应为 JSON({e}): {stdout}"));

    assert_eq!(report["session_state"], "active", "会话恢复为 active");
    // T7 claim:被杀回合自动续跑至终态(幂等续跑,ADR-0004 共识)
    assert_eq!(report["op_state"], "succeeded", "claim 重驱后回合完成");
    assert_eq!(report["interrupted_audit"], true, "中断审计事件在场");
    assert_eq!(report["interrupted_recovered"], 1);
    assert_eq!(report["replayed"], 0, "硬杀未破坏位点一致性,无修复窗口");

    // 被杀的 operation id 与 marker 一致(存在性恢复,ADR-0004 条件 5)
    let store = bm_persist::PersistStore::open(&dir_path).expect("重开");
    let ops = store
        .state()
        .query_rows("SELECT id, state FROM operations", &[])
        .expect("读操作");
    assert_eq!(ops.len(), 1);
    assert_eq!(
        ops[0]["id"],
        killed_op_id.trim(),
        "崩溃前的 operation 未消失(存在性恢复)"
    );
    assert_eq!(ops[0]["state"], "succeeded", "claim 续跑完成");
}
