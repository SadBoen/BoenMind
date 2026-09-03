//! 路径沙箱:allowed_roots 白名单 + 防逃逸。
//! 校验策略:候选路径(绝对化后)取最深已存在祖先做 canonicalize(操作系统
//! 侧吸收 `..`),余量按字面组件拼接回 canonical 基座,最后与各根做组件级
//! 前缀比对(`BoenMind` 不会误放行 `BoenMind2` 同名前缀)。

use std::path::{Path, PathBuf};

/// 剥离 Windows verbatim 前缀(`\\?\C:\..` → `C:\..`;`\\?\UNC\s\..` → `\\s\..`)
fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.as_os_str().to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p.to_path_buf()
    }
}

pub fn display_path(p: &Path) -> String {
    strip_verbatim(p).display().to_string()
}

#[derive(Debug, Clone)]
pub struct Roots {
    roots: Vec<PathBuf>,
}

impl Roots {
    /// 逐根 canonicalize(必须已存在);无效根剔除并告警,全无效 = 空沙箱
    /// (工具调用时统一报错,不拒启——配置修好「重载 MCP」即恢复)。
    pub fn new(raw: &[String]) -> Self {
        let mut roots = Vec::new();
        for r in raw {
            let cand = strip_verbatim(Path::new(r));
            match cand.canonicalize() {
                Ok(p) if p.is_dir() => roots.push(strip_verbatim(&p)),
                _ => eprintln!("[code-tools] allowed_root 无效(忽略):{r}"),
            }
        }
        Self { roots }
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// 把用户态路径解析为白名单内的绝对路径;越界/无法解析一律 Err。
    pub fn resolve(&self, input: &str) -> Result<PathBuf, String> {
        let input = input.trim();
        if input.is_empty() {
            return Err("路径参数为空".into());
        }
        if self.roots.is_empty() {
            return Err(
                "未配置 allowed_roots(设置页本插件配置填根目录,分号分隔多个;改后「重载 MCP」)"
                    .into(),
            );
        }
        let mut cand = strip_verbatim(Path::new(input));
        if !cand.is_absolute() {
            cand = self.roots[0].join(cand);
        }
        let mut base: Option<PathBuf> = None;
        for anc in cand.ancestors() {
            if anc.exists() {
                base = Some(anc.to_path_buf());
                break;
            }
        }
        let base = base.ok_or_else(|| format!("路径无法解析(盘符不存在?):{input}"))?;
        let canonical_base = strip_verbatim(
            &base
                .canonicalize()
                .map_err(|e| format!("canonicalize 失败:{e}"))?,
        );
        let rest = cand
            .strip_prefix(&base)
            .map_err(|e| format!("路径解析失败:{e}"))?;
        // 坑:join(空路径)会补尾随分隔符(PathBuf::join("")="x\"),目标
        // 恰为已存在文件本身时必踩(os error 267)——余量为空直接用基座。
        let final_path = if rest.as_os_str().is_empty() {
            canonical_base
        } else {
            canonical_base.join(rest)
        };
        for r in &self.roots {
            if final_path.starts_with(r) {
                return Ok(final_path);
            }
        }
        Err(format!(
            "路径越出 allowed_roots 白名单:{}(根:{})",
            display_path(&final_path),
            self.roots
                .iter()
                .map(|p| display_path(p))
                .collect::<Vec<_>>()
                .join(";")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots_of(dirs: &[&std::path::Path]) -> Roots {
        Roots::new(
            &dirs
                .iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn inside_root_resolves_and_normalizes() {
        let dir = tempfile::tempdir().expect("tmp");
        let r = roots_of(&[dir.path()]);
        let p = r.resolve("sub/file.txt").expect("相对路径挂第一根");
        assert!(p.starts_with(dir.path()));
        assert!(p.ends_with("sub/file.txt"));

        let p2 = r
            .resolve(
                &(dir
                    .path()
                    .join("./sub/../sub/file.txt")
                    .display()
                    .to_string()),
            )
            .expect("绝对路径 + 词法点");
        assert_eq!(p, p2);
    }

    #[test]
    fn dotdot_escape_rejected() {
        let dir = tempfile::tempdir().expect("tmp");
        let r = roots_of(&[dir.path()]);
        let out = r.resolve(r"..\..\..\Windows\win.ini");
        assert!(out.is_err(), "越界必须被拒:{out:?}");
        let out2 = r.resolve("sub/../../../../etc/passwd");
        assert!(out2.is_err(), "相对越界必须被拒:{out2:?}");
    }

    #[test]
    fn outside_absolute_rejected_even_if_exists() {
        let dir = tempfile::tempdir().expect("tmp");
        let other = tempfile::tempdir().expect("tmp2");
        let r = roots_of(&[dir.path()]);
        let out = r.resolve(&other.path().display().to_string());
        assert!(out.is_err(), "白名单外绝对路径必须被拒:{out:?}");
    }

    #[test]
    fn sibling_prefix_dir_not_confused() {
        let base = tempfile::tempdir().expect("tmp");
        let a = base.path().join("proj");
        let b = base.path().join("proj-secret");
        std::fs::create_dir_all(&a).expect("a");
        std::fs::create_dir_all(&b).expect("b");
        let r = roots_of(&[&a]);
        let out = r.resolve(&b.join("f.txt").display().to_string());
        assert!(
            out.is_err(),
            "组件级比对:proj-secret 不该被 proj 放行:{out:?}"
        );
        let ok = r.resolve(&a.join("f.txt").display().to_string());
        assert!(ok.is_ok());
    }

    #[test]
    fn write_into_new_subdir_resolves_via_existing_ancestor() {
        let dir = tempfile::tempdir().expect("tmp");
        let r = roots_of(&[dir.path()]);
        let target = "deep/new/nested/f.txt";
        let p = r.resolve(target).expect("父目录不存在也应可解析");
        assert!(p.ends_with(target));
    }

    #[test]
    fn empty_roots_and_empty_input_error() {
        let r = Roots::new(&[]);
        assert!(r.is_empty());
        assert!(r.resolve("x.txt").is_err());
        let dir = tempfile::tempdir().expect("tmp");
        let r2 = roots_of(&[dir.path()]);
        assert!(r2.resolve("   ").is_err());
    }
}
