//! P0(第四轮评审)验收:ID 水位扫描覆盖全部发号表——此前只扫
//! sessions/agents/operations,task/grant/approval/memory 发号漏计,
//! 重启后计数器回退会以 INSERT OR REPLACE 撞号覆写权力记录。

use bm_persist::{PersistStore, id_counter_hint};

#[test]
fn id_hint_covers_all_id_issuing_tables() {
    let dir = tempfile::tempdir().expect("临时目录");
    let store = PersistStore::open(dir.path()).expect("打开");
    let conn = store.state();

    // 直接向四张此前漏扫的表写入高号段行(模拟历史授权/任务/审批/记忆)
    let inserts = [
        "INSERT INTO tasks (id,title,state,created_by,task_epoch,payload,created_at,updated_at)
             VALUES ('task_00000000000000000000999901','t','running','x',0,'{}','t0','t0')",
        "INSERT INTO grants (id,audience,action,revocation_version,revoked,payload,created_at)
             VALUES ('grant_00000000000000000000999902','agent:x','model.invoke',0,0,'{}','t0')",
        "INSERT INTO approvals (id,operation_id,capability,principal,state,payload,created_at)
             VALUES ('appr_00000000000000000000999903','op_x','c','p','waiting_user','{}','t0')",
        "INSERT INTO memories (id,scope,tombstoned,payload,created_at)
             VALUES ('mem_00000000000000000000999904','memory:user',0,'{}','t0')",
    ];
    for sql in inserts {
        conn.query_rows(sql, &[]).expect("写入高号段行");
    }

    let hint = id_counter_hint(conn).expect("水位");
    assert!(
        hint >= 999_904,
        "水位必须覆盖 task/grant/approval/memory 发号,实际 {hint}"
    );
}
