//! 事件校验器：seq 连续 / JSON 无损 / ignorable 守卫 / surface 语义。
//!
//! 职责边界：校验器只判定语义，不持有状态；seq 的**分配**在存储层
//! （单写者锁内完成，见 [`crate::InMemoryEventStore`]），校验器负责
//! 防御性复核与重放流验证。

use bm_protocol::{ErrorCode, ProtocolError, SessionEvent, SESSION_FORMAT_VERSION};

/// 校验结果：事件对投影/重建的处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationOutcome {
    /// 通过
    Ok,
    /// 未知事件且 ignorable → 跳过（不参与重建）
    SkippedIgnorable,
}

pub struct EventValidator;

impl EventValidator {
    /// seq 连续性：head 存在时 next 必须 = head+1；否则 SeqGap/SeqDuplicate。
    pub fn check_next_seq(head: Option<u64>, next: u64) -> Result<(), ProtocolError> {
        match head {
            Some(h) if next == h + 1 => Ok(()),
            Some(h) if next == h => Err(ProtocolError::new(
                ErrorCode::SeqDuplicate,
                format!("seq {next} already at head {h}"),
            )),
            Some(h) => Err(ProtocolError::new(
                ErrorCode::SeqGap,
                format!("expected seq {}, got {next} (head {h})", h + 1),
            )),
            None if next == 1 => Ok(()),
            None => Err(ProtocolError::new(
                ErrorCode::SeqGap,
                format!("expected first seq 1, got {next}"),
            )),
        }
    }

    /// JSON 无损：信封序列化 → 反序列化 → 序列化，字节必须一致。
    /// （默认 serde_json 映射排序稳定，roundtrip 是 lossless 的充分检查）
    pub fn verify_lossless(ev: &SessionEvent) -> Result<(), ProtocolError> {
        let json = serde_json::to_string(ev).map_err(|e| {
            ProtocolError::new(ErrorCode::InvalidArgument, format!("serialize: {e}"))
        })?;
        let back: SessionEvent = serde_json::from_str(&json).map_err(|e| {
            ProtocolError::new(ErrorCode::InvalidArgument, format!("deserialize: {e}"))
        })?;
        let json2 = serde_json::to_string(&back).map_err(|e| {
            ProtocolError::new(ErrorCode::InvalidArgument, format!("reserialize: {e}"))
        })?;
        if json != json2 {
            return Err(ProtocolError::new(
                ErrorCode::InvalidArgument,
                "event JSON not lossless (roundtrip bytes differ)",
            ));
        }
        Ok(())
    }

    /// ignorable 守卫：未知且必需的事件 → 拒绝重建（D2，防旧版本
    /// 静默读坏新日志）。返回 Ok(true) = 未知且 ignorable → 跳过。
    pub fn guard_unknown(ignorable: bool) -> Result<ValidationOutcome, ProtocolError> {
        if ignorable {
            Ok(ValidationOutcome::SkippedIgnorable)
        } else {
            Err(ProtocolError::new(
                ErrorCode::UnknownRequiredEvent,
                "unknown event with ignorable=false: refusing to rebuild state",
            ))
        }
    }

    /// 格式版本检查（写者决定 bump）：version != 当前 → 拒绝重建，
    /// 走迁移链（当前无迁移，报 format_version_mismatch）。
    pub fn check_version(ev: &SessionEvent) -> Result<(), ProtocolError> {
        if ev.version != SESSION_FORMAT_VERSION {
            return Err(ProtocolError::new(
                ErrorCode::FormatVersionMismatch,
                format!(
                    "event format v{} != current v{SESSION_FORMAT_VERSION} (migration required)",
                    ev.version
                ),
            ));
        }
        Ok(())
    }

    /// A7 migrate-on-continue：把任意低版本事件逐级迁移到当前版本（骨架）。
    ///
    /// - version == 当前：原样通过；
    /// - version < 当前：沿 [`bm_protocol::FORMAT_MIGRATIONS`] 逐级 apply；
    ///   缺步骤 → MigrationUnavailable（当前无任何步骤，v0 数据仍拒绝重建，
    ///   与版本化之前语义一致）；
    /// - version > 当前：旧程序读新数据 → FormatVersionMismatch。
    ///
    /// 只做内存迁移（读时升级），不写回存储——下次 append 自然以当前版本落盘。
    pub fn migrate(ev: SessionEvent) -> Result<SessionEvent, ProtocolError> {
        if ev.version > SESSION_FORMAT_VERSION {
            return Err(ProtocolError::new(
                ErrorCode::FormatVersionMismatch,
                format!(
                    "event format v{} newer than current v{SESSION_FORMAT_VERSION} (old reader)",
                    ev.version
                ),
            ));
        }
        let mut ev = ev;
        while ev.version < SESSION_FORMAT_VERSION {
            let from = ev.version;
            let step = bm_protocol::FORMAT_MIGRATIONS.get(from as usize).ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::MigrationUnavailable,
                    format!("no migration path for event format v{from} → v{}", from + 1),
                )
            })?;
            ev = step(ev)?;
            if ev.version <= from {
                return Err(ProtocolError::new(
                    ErrorCode::MigrationUnavailable,
                    format!("migration v{from} did not advance version"),
                ));
            }
        }
        Ok(ev)
    }

    /// 重放流验证：seq 严格递增且无重复（跨事件防御性检查）。
    ///
    /// 注意：不要求"连续"——未知 ignorable 事件被跳过会产生合法空洞
    /// （D2 语义）；写入路径的连续性由存储层单写者 + check_next_seq 保证。
    pub fn verify_replay(evs: &[SessionEvent]) -> Result<(), ProtocolError> {
        let mut prev: Option<u64> = None;
        for ev in evs {
            match prev {
                Some(p) if ev.seq.as_u64() <= p => {
                    return Err(ProtocolError::new(
                        ErrorCode::SeqDuplicate,
                        format!("replay seq {} not strictly after {}", ev.seq.as_u64(), p),
                    ));
                }
                _ => {}
            }
            prev = Some(ev.seq.as_u64());
        }
        Ok(())
    }

    /// surface 语义：Replace 区间合法性（start <= end，由 SurfaceOp 保证）；
    /// 此处补充跨事件检查：区间端点必须是已出现过的 seq（防幻影遮蔽）。
    pub fn check_replace_interval(start: u64, end: u64, max_seen: u64) -> Result<(), ProtocolError> {
        if start > end {
            return Err(ProtocolError::new(
                ErrorCode::SurfaceViolation,
                format!("replace interval {start}..{end} inverted"),
            ));
        }
        if end > max_seen {
            return Err(ProtocolError::new(
                ErrorCode::SurfaceViolation,
                format!("replace interval {start}..{end} exceeds head {max_seen}"),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_seq_rules() {
        assert!(EventValidator::check_next_seq(None, 1).is_ok());
        assert!(EventValidator::check_next_seq(Some(3), 4).is_ok());
        assert_eq!(EventValidator::check_next_seq(Some(3), 3).unwrap_err().code(), ErrorCode::SeqDuplicate);
        assert_eq!(EventValidator::check_next_seq(Some(3), 5).unwrap_err().code(), ErrorCode::SeqGap);
        assert_eq!(EventValidator::check_next_seq(None, 2).unwrap_err().code(), ErrorCode::SeqGap);
    }

    #[test]
    fn guard_unknown_rules() {
        assert_eq!(
            EventValidator::guard_unknown(true).unwrap(),
            ValidationOutcome::SkippedIgnorable
        );
        assert_eq!(
            EventValidator::guard_unknown(false).unwrap_err().code(),
            ErrorCode::UnknownRequiredEvent
        );
    }

    #[test]
    fn verify_replay_strictly_increasing_allows_skips() {
        // ignorable 跳过后留空洞是合法的（seq 1, 3）
        let ev = |seq: u64| SessionEvent {
            version: bm_protocol::SESSION_FORMAT_VERSION,
            seq: bm_protocol::SeqNo::new(seq),
            session_id: bm_protocol::SessionId::new("s"),
            branch_id: bm_protocol::BranchId::new("main"),
            time: 1,
            kind: bm_protocol::EventKind::Core(bm_protocol::CoreEvent::TurnStart { turn: 1 }),
            ignorable: false,
            surface_op: None,
            source_seqs: None,
        };
        assert!(EventValidator::verify_replay(&[ev(1), ev(3)]).is_ok());
        // 重复/回退拒绝
        assert!(EventValidator::verify_replay(&[ev(2), ev(2)]).is_err());
        assert!(EventValidator::verify_replay(&[ev(3), ev(1)]).is_err());
    }

    #[test]
    fn version_mismatch_rejected() {
        // 当前版本通过；旧格式（version=0）拒绝重建
        let mut ev = SessionEvent {
            version: SESSION_FORMAT_VERSION,
            seq: bm_protocol::SeqNo::new(1),
            session_id: bm_protocol::SessionId::new("s"),
            branch_id: bm_protocol::BranchId::new("main"),
            time: 1,
            kind: bm_protocol::EventKind::Core(bm_protocol::CoreEvent::TurnStart { turn: 1 }),
            ignorable: false,
            surface_op: None,
            source_seqs: None,
        };
        assert!(EventValidator::check_version(&ev).is_ok());
        ev.version = 0;
        let err = EventValidator::check_version(&ev).unwrap_err();
        assert_eq!(err.code(), ErrorCode::FormatVersionMismatch);
    }

    #[test]
    fn migrate_current_version_is_identity() {
        let ev = SessionEvent {
            version: SESSION_FORMAT_VERSION,
            seq: bm_protocol::SeqNo::new(1),
            session_id: bm_protocol::SessionId::new("s"),
            branch_id: bm_protocol::BranchId::new("main"),
            time: 1,
            kind: bm_protocol::EventKind::Core(bm_protocol::CoreEvent::TurnStart { turn: 1 }),
            ignorable: false,
            surface_op: None,
            source_seqs: None,
        };
        let back = EventValidator::migrate(ev.clone()).unwrap();
        assert_eq!(back, ev, "当前版本事件原样通过（A7 骨架）");
    }

    #[test]
    fn migrate_without_path_rejected() {
        // v0 是版本化之前的旧数据：迁移链无 0→1 步骤 → MigrationUnavailable
        // （拒绝重建与版本化之前的语义一致；首个真实迁移落地后此测试改走链）
        let mut ev = SessionEvent {
            version: 0,
            seq: bm_protocol::SeqNo::new(1),
            session_id: bm_protocol::SessionId::new("s"),
            branch_id: bm_protocol::BranchId::new("main"),
            time: 1,
            kind: bm_protocol::EventKind::Core(bm_protocol::CoreEvent::TurnStart { turn: 1 }),
            ignorable: false,
            surface_op: None,
            source_seqs: None,
        };
        let err = EventValidator::migrate(ev.clone()).unwrap_err();
        assert_eq!(err.code(), ErrorCode::MigrationUnavailable);

        // version > 当前：旧程序读新数据 → FormatVersionMismatch
        ev.version = SESSION_FORMAT_VERSION + 1;
        let err = EventValidator::migrate(ev).unwrap_err();
        assert_eq!(err.code(), ErrorCode::FormatVersionMismatch);
    }

    #[test]
    fn replace_interval_rules() {
        assert!(EventValidator::check_replace_interval(2, 5, 10).is_ok());
        assert_eq!(
            EventValidator::check_replace_interval(5, 2, 10).unwrap_err().code(),
            ErrorCode::SurfaceViolation
        );
        assert_eq!(
            EventValidator::check_replace_interval(8, 9, 5).unwrap_err().code(),
            ErrorCode::SurfaceViolation
        );
    }
}
