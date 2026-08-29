//! 令牌文件(合同库 surface/auth.v0_1):256bit 随机数 base64url(43 字符)。
//! 首次启动生成;POSIX 0600,Windows 依赖父目录用户 ACL(best-effort)。

use std::io::Write;
use std::path::Path;

/// 读取既有令牌;不存在则生成并写入。
pub fn load_or_create(dir: &Path) -> std::io::Result<String> {
    let path = dir.join("token");
    if path.exists() {
        let t = std::fs::read_to_string(&path)?.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    let token = b64url_nopad(&bytes);
    let mut f = std::fs::File::create(&path)?;
    f.write_all(token.as_bytes())?;
    f.flush()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(token)
}

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// 标准 base64url 无填充编码。
fn b64url_nopad(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64URL[(n >> 18) as usize & 63] as char);
        out.push(B64URL[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(B64URL[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(B64URL[n as usize & 63] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_format_matches_auth_contract() {
        let dir = tempfile::tempdir().expect("临时目录");
        let t = load_or_create(dir.path()).expect("生成");
        assert_eq!(t.len(), 43, "256bit base64url 无填充 = 43 字符");
        assert!(
            t.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        // 幂等:同目录再读为同一令牌
        assert_eq!(load_or_create(dir.path()).expect("再读"), t);
    }
}
