//! 配置域 JSON 文件读写收口(issue #38):providers/skills/roles/mcp 四域
//! 此前各持一套 read_to_string→from_str→create_dir_all→atomic_write 样板
//! (skills 连写盘都是两处内联)。本模块收口为两个原语,域内只保留形状
//! 提取与 NotFound 回退。口径不变:
//! - 缺文件 = Missing(域内回退默认文档/空库);
//! - JSON 损坏 = Err 拒绝(2026-09-07 复核批:防半损坏文件被下一次保存
//!   整库覆写);
//! - 错误文案由调用方传域前缀,与收口前逐字一致(crlf 统一走
//!   config_store::crlf,P2 评审口径)。

use serde_json::Value;
use std::path::Path;

pub(crate) enum JsonRead {
    /// 文件存在且为合法 JSON。
    Value(Value),
    /// 文件不存在(域内回退各自默认文档/空库)。
    Missing,
}

/// 读 JSON 文件:NotFound = Missing;其余读错误与解析错误按域前缀报错。
pub(crate) fn read_json_file(
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

/// 写 JSON 文件:建父目录 + pretty + crlf + atomic_write(唯一 tmp 名)。
pub(crate) fn write_json_file(
    path: &Path,
    value: &Value,
    write_err_prefix: &str,
) -> Result<(), String> {
    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return Err(format!("目录创建失败: {e}"));
    }
    let text = crate::config_store::crlf(
        serde_json::to_string_pretty(value).map_err(|e| format!("序列化失败: {e}"))?,
    );
    bm_core::ports::persist::atomic_write(path, text.as_bytes())
        .map_err(|e| format!("{write_err_prefix}: {e}"))
}
