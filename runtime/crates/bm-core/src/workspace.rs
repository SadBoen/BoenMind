//! W8(ADR-0018):工作区注册表读取面。
//! 唯一数据源 = `<data_dir>/config/workspaces.json`(管理面负责写盘,
//! 核心只读;ADR-0012 配置文件口径)。id 为不透明短 id,路径解析只在
//! 服务器侧发生——浏览器/模型不得以任意绝对路径当权限凭据(ADR-0006)。

use std::path::Path;

/// 注册表条目(id 不透明;path 为登记时的规范化绝对路径文本)。
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceEntry {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// 默认条目 id(管理面首次读取时按现役文件浏览根播种)。
pub const DEFAULT_WORKSPACE_ID: &str = "default";

fn workspaces_file(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("config").join("workspaces.json")
}

/// 读注册表(缺文件/坏文件 = 空表,不阻断;与 roles.json 同款宽容读)。
pub fn read_workspaces(data_dir: &Path) -> Vec<WorkspaceEntry> {
    let Ok(text) = std::fs::read_to_string(workspaces_file(data_dir)) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    v["workspaces"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    Some(WorkspaceEntry {
                        id: e["id"].as_str()?.to_string(),
                        name: e["name"].as_str().unwrap_or("").to_string(),
                        path: e["path"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 按 id 解析工作区(未登记/路径为空 = None)。
pub fn resolve(data_dir: &Path, id: &str) -> Option<WorkspaceEntry> {
    read_workspaces(data_dir)
        .into_iter()
        .find(|w| w.id == id && !w.path.is_empty())
}

/// 会话绑定的校验入口:登记表中存在即合法。
pub fn is_registered(data_dir: &Path, id: &str) -> bool {
    resolve(data_dir, id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_workspaces(dir.path()).is_empty());
        assert!(!is_registered(dir.path(), "default"));
        assert!(resolve(dir.path(), "default").is_none());
    }

    #[test]
    fn reads_entries_and_resolves_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("workspaces.json"),
            serde_json::json!({
                "workspaces": [
                    {"id": "default", "name": "默认工作区", "path": "C:/ws"},
                    {"id": "ws_abc", "name": "项目甲", "path": "D:/proj/a"}
                ]
            })
            .to_string(),
        )
        .unwrap();
        assert!(is_registered(dir.path(), "ws_abc"));
        let e = resolve(dir.path(), "ws_abc").unwrap();
        assert_eq!(e.name, "项目甲");
        assert_eq!(e.path, "D:/proj/a");
        assert!(!is_registered(dir.path(), "ws_nope"));
    }

    #[test]
    fn corrupt_file_yields_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("workspaces.json"), "{broken").unwrap();
        assert!(read_workspaces(dir.path()).is_empty());
    }
}
