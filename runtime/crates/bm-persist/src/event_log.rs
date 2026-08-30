//! JSONL 事件日志:一行一条 event_envelope,追加写 + 按行重放 + 前缀截断(压实)。
//!
//! 纪律:append 必须 flush(可配 fsync);truncate_prefix 通过「临时文件重写 + rename」
//! 完成前缀压实,失败时原文件不动(压实是优化,绝不引入数据丢失)。

use crate::error::{StoreError, StoreResult};
use bm_contract::events::EventEnvelope;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct JsonlEventLog {
    path: PathBuf,
    file: Mutex<File>,
}

impl JsonlEventLog {
    /// 打开(不存在则创建空日志)。
    pub fn open(path: PathBuf) -> StoreResult<Self> {
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            file: Mutex::new(file),
        })
    }

    /// 追加一条事件并 flush。seq 单调性由调用方(单写者核心循环)保证;
    /// 本层只防御性检查「新 seq 严格大于日志末尾」。
    pub fn append(&self, event: &EventEnvelope, fsync: bool) -> StoreResult<()> {
        let last = self.last_seq()?;
        if event.event_seq <= last {
            return Err(StoreError::Corrupt {
                seq: event.event_seq,
                reason: format!("seq 必须严格大于日志末尾 {last}(INV-3)"),
            });
        }
        let mut line = serde_json::to_string(event).map_err(|e| StoreError::Corrupt {
            seq: event.event_seq,
            reason: format!("序列化失败: {e}"),
        })?;
        line.push('\n');
        let mut f = self.file.lock().expect("锁未中毒");
        f.write_all(line.as_bytes())?;
        f.flush()?;
        if fsync {
            f.sync_all()?;
        }
        Ok(())
    }

    /// seq > since 的事件(全量扫描;压实后即增量)。
    pub fn replay_since(&self, since_seq: u64) -> StoreResult<Vec<EventEnvelope>> {
        let file = File::open(&self.path)?;
        let mut out = Vec::new();
        for (idx, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: EventEnvelope =
                serde_json::from_str(&line).map_err(|e| StoreError::Corrupt {
                    seq: idx as u64 + 1,
                    reason: format!("行解析失败: {e}"),
                })?;
            if event.event_seq > since_seq {
                out.push(event);
            }
        }
        Ok(out)
    }

    /// 日志末尾 seq(空日志 = 0)。
    pub fn last_seq(&self) -> StoreResult<u64> {
        let file = File::open(&self.path)?;
        let mut last = 0u64;
        for (idx, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: EventEnvelope =
                serde_json::from_str(&line).map_err(|e| StoreError::Corrupt {
                    seq: idx as u64 + 1,
                    reason: format!("行解析失败: {e}"),
                })?;
            last = last.max(event.event_seq);
        }
        Ok(last)
    }

    /// 压实:截断 seq ≤ up_to 的前缀。先快照成功后才允许调用(M2 规格 §2)。
    /// 通过临时文件重写 + 原子改名;任何失败原文件保持不变。
    /// 外部审计 X-03(P1):日志首序号(空文件 = 0)。
    /// 恢复判据:首序号 > 1 表示前缀已压实,自动重建将丢失前缀事实。
    pub fn first_seq(&self) -> StoreResult<u64> {
        let file = File::open(&self.path)?;
        for (idx, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: EventEnvelope =
                serde_json::from_str(&line).map_err(|e| StoreError::Corrupt {
                    seq: idx as u64 + 1,
                    reason: format!("行解析失败: {e}"),
                })?;
            return Ok(event.event_seq);
        }
        Ok(0)
    }

    pub fn truncate_prefix(&self, up_to_seq: u64) -> StoreResult<usize> {
        let file = File::open(&self.path)?;
        let mut kept: Vec<String> = Vec::new();
        let mut dropped = 0usize;
        for (idx, line) in BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: EventEnvelope =
                serde_json::from_str(&line).map_err(|e| StoreError::Corrupt {
                    seq: idx as u64 + 1,
                    reason: format!("行解析失败: {e}"),
                })?;
            if event.event_seq <= up_to_seq {
                dropped += 1;
            } else {
                kept.push(line);
            }
        }
        if dropped == 0 {
            return Ok(0);
        }
        let tmp = self.path.with_extension("jsonl.tmp");
        {
            let mut f = File::create(&tmp)?;
            for line in &kept {
                writeln!(f, "{line}")?;
            }
            f.flush()?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        // 重打开句柄(旧句柄指向已被改名的文件)
        let new_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        *self.file.lock().expect("锁未中毒") = new_file;
        Ok(dropped)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bm_contract::events::EventType;

    fn ev(seq: u64) -> EventEnvelope {
        EventEnvelope::new_unchecked(
            seq,
            EventType::RuntimeStarted,
            bm_contract::timestamp::now(),
            None,
            None,
            None,
            serde_json::json!({}),
        )
    }

    #[test]
    fn append_replay_and_monotonic_guard() {
        let dir = tempfile::tempdir().expect("临时目录");
        let log = JsonlEventLog::open(dir.path().join("events.jsonl")).expect("打开");
        assert_eq!(log.last_seq().expect("空日志"), 0);

        log.append(&ev(1), false).expect("追加 1");
        log.append(&ev(2), false).expect("追加 2");
        // 乱序/重复 seq 必须被拒
        assert!(log.append(&ev(2), false).is_err(), "seq 必须严格递增");

        assert_eq!(log.last_seq().expect("末尾"), 2);
        let all = log.replay_since(0).expect("全量");
        assert_eq!(all.len(), 2);
        let tail = log.replay_since(1).expect("增量");
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].event_seq, 2);
    }

    #[test]
    fn truncate_prefix_compacts_and_preserves_suffix() {
        let dir = tempfile::tempdir().expect("临时目录");
        let log = JsonlEventLog::open(dir.path().join("events.jsonl")).expect("打开");
        for seq in 1..=5 {
            log.append(&ev(seq), false).expect("追加");
        }
        let dropped = log.truncate_prefix(3).expect("压实");
        assert_eq!(dropped, 3);
        assert_eq!(log.last_seq().expect("末尾"), 5);
        let all = log.replay_since(0).expect("重放");
        assert_eq!(
            all.iter().map(|e| e.event_seq).collect::<Vec<_>>(),
            vec![4, 5]
        );
        // 压实后仍可继续追加(句柄已重开)
        log.append(&ev(6), false).expect("压实后追加");
        assert_eq!(log.last_seq().expect("末尾"), 6);
    }
}
