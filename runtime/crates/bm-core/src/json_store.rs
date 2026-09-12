//! 配置文件 JSON 读写单一原语(2026-09-12,架构评估收口 #71)。
//!
//! 此前「读一个 config JSON」有**三种实现**:本 crate 内多处手写
//! `read_to_string().ok()? + from_str().ok()?`(roles/workspace/limits)、
//! surface 的 `webadmin::json_store`、surface 的 `config_store::read_file_strict`
//! ——同一件事三份样板,且损坏策略散落各处,已出现「同一 roles.json 被两套
//! 策略解析」的漂移风险。
//!
//! 收口为**一份实现 + 两种显式命名的策略**(策略差异是刻意的,不是要抹平):
//!
//! - [`read_json_file`]:严格策略。**仅 NotFound = [`JsonRead::Missing`]**,
//!   其余 IO 错误与 JSON 损坏一律 `Err`。用于**写入前置读取**——损坏必须
//!   让调用方看见,否则下一次保存会把半损坏文件整库覆写(数据丢失)。
//! - [`read_json_lenient`]:宽容策略。缺文件或损坏一律 `None`。用于**只读
//!   消费**——读不到就回退默认,不阻塞主流程(如只读解析角色/工作区)。
//!
//! 写侧 [`write_json_file`] 单一实现(建父目录 + pretty + [`crlf`] +
//! 原子覆盖写)。[`crlf`] 亦在此单源,`config_store` 仅 re-export 保持旧路径。

use serde_json::Value;
use std::path::Path;

/// 配置文件读取结果(严格策略):文件存在且合法 vs 不存在。
#[derive(Debug)]
pub enum JsonRead {
    /// 文件存在且为合法 JSON。
    Value(Value),
    /// 文件不存在(调用方回退各自默认文档/空库)。
    Missing,
}

/// 读 JSON 文件(严格):NotFound = [`JsonRead::Missing`];其余读错误与
/// 解析错误按调用方给的前缀报错。错误文案由调用方传域前缀(与收口前逐字
/// 一致)——故本原语不内置任何业务措辞。
pub fn read_json_file(
    path: &Path,
    read_err_prefix: &str,
    corrupt_prefix: &str,
) -> Result<JsonRead, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(JsonRead::Missing),
        Err(e) => return Err(format!("{read_err_prefix}: {e}")),
    };
    serde_json::from_str(&raw)
        .map(JsonRead::Value)
        .map_err(|e| format!("{corrupt_prefix}: {e}"))
}

/// 读 JSON 文件(宽容):缺文件或损坏/不可读一律 `None`。
/// 用于只读消费面(读不到即回退默认),**不得**用于写入前置读取
/// (那会把损坏静默当空,随后整库覆写)。
pub fn read_json_lenient(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 写 JSON 文件:建父目录 + pretty + [`crlf`] + 原子覆盖写(唯一 tmp 名)。
pub fn write_json_file(path: &Path, value: &Value, write_err_prefix: &str) -> Result<(), String> {
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return Err(format!("目录创建失败: {e}"));
    }
    let text = crlf(serde_json::to_string_pretty(value).map_err(|e| format!("序列化失败: {e}"))?);
    crate::ports::persist::atomic_write(path, text.as_bytes())
        .map_err(|e| format!("{write_err_prefix}: {e}"))
}

/// pretty JSON → CRLF 文本(Windows 人可读口径;配置写入共用单源)。
pub fn crlf(pretty: String) -> String {
    pretty.replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strict_distinguishes_missing_corrupt_and_valid() {
        let dir = tempfile::tempdir().expect("临时目录");
        let missing = dir.path().join("nope.json");
        assert!(matches!(
            read_json_file(&missing, "读失败", "损坏"),
            Ok(JsonRead::Missing)
        ));

        let corrupt = dir.path().join("bad.json");
        std::fs::write(&corrupt, b"{not json").expect("写");
        let err = read_json_file(&corrupt, "读失败", "损坏").expect_err("损坏必须报错");
        assert!(err.starts_with("损坏: "), "{err}");

        let ok = dir.path().join("ok.json");
        write_json_file(&ok, &json!({"a": 1}), "写失败").expect("写");
        match read_json_file(&ok, "读失败", "损坏").expect("读") {
            JsonRead::Value(v) => assert_eq!(v["a"], json!(1)),
            JsonRead::Missing => panic!("不应为 Missing"),
        }
    }

    #[test]
    fn lenient_swallows_missing_and_corrupt() {
        let dir = tempfile::tempdir().expect("临时目录");
        assert!(read_json_lenient(&dir.path().join("nope.json")).is_none());
        let corrupt = dir.path().join("bad.json");
        std::fs::write(&corrupt, b"{not json").expect("写");
        assert!(read_json_lenient(&corrupt).is_none(), "宽容策略不得报错");
    }

    #[test]
    fn write_roundtrip_uses_crlf() {
        let dir = tempfile::tempdir().expect("临时目录");
        let p = dir.path().join("nested").join("v.json");
        write_json_file(&p, &json!({"k": "v"}), "写失败").expect("建父目录并写");
        let raw = std::fs::read_to_string(&p).expect("读回");
        assert!(raw.contains("\r\n"), "写入须为 CRLF: {raw:?}");
    }
}
