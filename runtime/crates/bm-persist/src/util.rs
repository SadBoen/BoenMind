//! 落盘小工具:配置类文件的原子写(外部评审 2026-09-03 P3)。
//!
//! `std::fs::write` 原地截断重写,断电/磁盘满/进程崩溃会留下半截文件,
//! 配置损坏 = 下次启动即挂。此处统一「临时文件 + flush + fsync + rename」:
//! 任意时刻盘上只有完整旧文件或完整新文件(rename 单原子)。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 临时文件序号(P1-15,2026-09-07 架构评审):此前固定 `{path}.tmp`,两个
/// 并发写同一目标会互踩同一临时文件造成丢失更新;现按进程内序号唯一化。
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 原子覆盖写。目标父目录不存在则创建;Windows 上 rename 经
/// MoveFileExW(REPLACE_EXISTING) 可覆盖既有文件。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(format!(".tmp{}", TMP_SEQ.fetch_add(1, Ordering::Relaxed)));
    let tmp = PathBuf::from(tmp_name);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// 流式过滤重写(2026-09-06 会话删除配套):逐行读源文件,`drop_line`
/// 命中的行不写入临时文件,随后 fsync + rename 原子替换。内存只占缓冲,
/// 不整文件载入;任意时刻盘上只有完整旧文件或完整新文件。
/// 返回剔除的行数。源文件不存在 = Ok(0)(无记录可擦)。
pub use bm_core::ports::persist::filter_lines_atomic;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_survives() {
        let dir = tempfile::tempdir().expect("临时目录");
        let p = dir.path().join("cfg").join("a.json");
        atomic_write(&p, b"v1").expect("首写");
        assert_eq!(std::fs::read(&p).expect("读"), b"v1");
        atomic_write(&p, b"v2-longer-content").expect("覆盖写");
        assert_eq!(std::fs::read(&p).expect("读"), b"v2-longer-content");
        // 临时文件不留痕(tmp 名带序号,扫描目录断言)
        let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("cfg"))
            .expect("列目录")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件: {leftovers:?}");
    }

    #[test]
    fn filter_lines_atomic_drops_and_keeps_order() {
        let dir = tempfile::tempdir().expect("临时目录");
        let p = dir.path().join("log.jsonl");
        std::fs::write(
            &p, "a
b
c
",
        )
        .expect("写");
        let dropped = filter_lines_atomic(&p, |l| l.contains("b")).expect("过滤");
        assert_eq!(dropped, 1);
        assert_eq!(
            std::fs::read(&p).expect("读"),
            b"a
c
"
        );
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("列目录")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件: {leftovers:?}");
        // 文件不存在 = 0
        let dropped2 =
            filter_lines_atomic(&dir.path().join("nope.jsonl"), |_| true).expect("不存在");
        assert_eq!(dropped2, 0);
    }
}
