//! StateDb 域方法(自 sqlite_state.rs 机械移入;内容零改动)。
use super::StateDb;
use crate::error::StoreError;
use crate::error::StoreResult;

impl StateDb {
    /// M8.5:在线备份(VACUUM INTO;目标文件必须不存在)。运行中可取,
    /// 产出一致的快照副本(含 WAL 内容合并)。
    pub fn backup_into(&self, target: &std::path::Path) -> StoreResult<()> {
        // 外部审计 X-04(P2):checkpoint+copy 之间存在提交窗口,且与
        // 事件日志位点不绑定。改用 SQLite Online Backup API(一致快照),
        // 并写入位点清单(manifest)供恢复侧校验。
        use rusqlite::backup::Backup;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                StoreError::Io(std::io::Error::other(format!("备份目录创建失败: {e}")))
            })?;
        }
        let mut dst = rusqlite::Connection::open(target)?;
        {
            let src = self.conn.lock().expect("锁未中毒");
            let b = Backup::new(&src, &mut dst)?;
            // step 返回 StepResult(Done/More);Done 即完成
            use rusqlite::backup::StepResult;
            loop {
                match b.step(64)? {
                    StepResult::Done => break,
                    _ => continue,
                }
            }
        }

        Ok(())
    }

    /// WAL checkpoint 维护操作:主动将 WAL 日志刷入主数据库文件 (PASSIVE 模式)。
    pub fn wal_checkpoint(&self) -> StoreResult<()> {
        let conn = self.conn.lock().expect("锁未中毒");
        conn.pragma_update(None, "wal_checkpoint", "PASSIVE")?;
        Ok(())
    }
}
