//! 落盘小工具:配置类文件的原子写(外部评审 2026-09-03 P3)。
//!
//! `std::fs::write` 原地截断重写,断电/磁盘满/进程崩溃会留下半截文件,
//! 配置损坏 = 下次启动即挂。此处统一「临时文件 + flush + fsync + rename」:
//! 任意时刻盘上只有完整旧文件或完整新文件(rename 单原子)。

use std::io::Write;
use std::path::{Path, PathBuf};

/// 原子覆盖写。目标父目录不存在则创建;Windows 上 rename 经
/// MoveFileExW(REPLACE_EXISTING) 可覆盖既有文件。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
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
pub fn filter_lines_atomic<F>(path: &Path, drop_line: F) -> std::io::Result<usize>
where
    F: Fn(&str) -> bool,
{
    use std::io::{BufRead, BufReader};
    let Ok(reader) = std::fs::File::open(path) else {
        return Ok(0); // 文件不存在 = 无可擦
    };
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".purge.tmp");
    let tmp = PathBuf::from(tmp_name);
    let mut out = std::fs::File::create(&tmp)?;
    let mut dropped = 0usize;
    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { break };
        if drop_line(&line) {
            dropped += 1;
            continue;
        }
        out.write_all(line.as_bytes())?;
        out.write_all(
            b"
",
        )?;
    }
    out.flush()?;
    out.sync_all()?;
    drop(out);
    std::fs::rename(&tmp, path)?;
    Ok(dropped)
}

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
        // 临时文件不留痕
        assert!(!dir.path().join("cfg").join("a.json.tmp").exists());
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
        assert!(!dir.path().join("log.jsonl.purge.tmp").exists());
        // 文件不存在 = 0
        let dropped2 =
            filter_lines_atomic(&dir.path().join("nope.jsonl"), |_| true).expect("不存在");
        assert_eq!(dropped2, 0);
    }
}
