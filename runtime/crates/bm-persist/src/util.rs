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
}
