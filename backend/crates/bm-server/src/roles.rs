//! 角色注入（角色定义插件的宿主挂点）。
//!
//! 分工（对齐记忆注入 bm-memory 的"插件管策略、宿主给挂点"）：
//! - 插件侧（backend/plugins/role/）：管理 `~/.boenmind/roles.json`
//!   （角色增删改查 + 当前激活 id），经宿主 write/read 工具真实落盘；
//! - 宿主侧（本模块）：每次模型请求前读取当前激活角色的 prompt，追加进
//!   system 消息——角色切换即时生效（无需重建会话，对齐记忆注入挂点）。
//!
//! 文件结构：`{ "active": "<id>|null", "roles": [{ "id", "name", "prompt" }] }`
//! 宿主只读不写：文件缺失/损坏/未激活 → 静默不注入（fail-open，不阻断对话）；
//! 角色注入是有界字符原则（上限 [`MAX_ROLE_PROMPT_CHARS`]），防大文本进上下文。

use std::path::{Path, PathBuf};

/// 角色数据文件名（位于 `~/.boenmind/` 下，与插件侧约定一致）。
pub const ROLES_FILE: &str = "roles.json";
/// 角色 prompt 注入上限（字符，Unicode 计；插件侧写入校验同值）。
pub const MAX_ROLE_PROMPT_CHARS: usize = 2000;

/// 角色注入器：只读 roles.json（插件侧写入），把激活角色追加进 payload。
/// 无内部状态（每次请求读文件），可跨会话共享。
#[derive(Debug, Clone)]
pub struct RoleInjector {
    path: PathBuf,
}

impl RoleInjector {
    /// 指向 `app_dir()/roles.json`（与插件侧 os.homedir()/.boenmind 同位置）。
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// 把当前激活角色块追加进 payload 的 system 消息（有则改、无则插，
    /// 与 bm-memory 的注入同款）。无角色/读失败 → 不改 payload。
    pub fn inject(&self, payload: &mut serde_json::Value) {
        let Some((name, prompt)) = active_role(&self.path) else {
            return;
        };
        let block = format!("\n\n[当前角色：{name}]\n{prompt}");
        let Some(messages) = payload
            .get_mut("messages")
            .and_then(|m| m.as_array_mut())
        else {
            return;
        };
        if let Some(system) = messages
            .iter_mut()
            .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
            && let Some(content) = system.get_mut("content")
            && let Some(s) = content.as_str()
        {
            *content = serde_json::json!(format!("{s}{block}"));
        } else {
            messages.insert(0, serde_json::json!({ "role": "system", "content": block }));
        }
    }
}

/// 读取激活角色 (name, prompt)。文件缺失/损坏/未激活/找不到 → None。
fn active_role(path: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let active = json.get("active")?.as_str()?;
    if active.is_empty() {
        return None;
    }
    let roles = json.get("roles")?.as_array()?;
    let role = roles.iter().find(|r| r.get("id").and_then(serde_json::Value::as_str) == Some(active))?;
    let name = role.get("name").and_then(serde_json::Value::as_str)?;
    let prompt = role.get("prompt").and_then(serde_json::Value::as_str)?;
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return None;
    }
    // 注入是有界字符原则：超上限截断（插件侧写入已校验，此处兜底防手改文件）
    let prompt: String = prompt.chars().take(MAX_ROLE_PROMPT_CHARS).collect();
    Some((name.to_string(), prompt))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("bm-role-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn payload(messages: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "messages": messages })
    }

    #[test]
    fn injects_active_role_into_existing_system_message() {
        let path = temp_file("a.json");
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"{"active":"architect","roles":[{"id":"architect","name":"架构师","prompt":"你是资深架构师"}]}"#,
        )
        .unwrap();
        let inj = RoleInjector::new(path);
        let mut p = payload(serde_json::json!([
            { "role": "system", "content": "基础提示" },
            { "role": "user", "content": "hi" },
        ]));
        inj.inject(&mut p);
        let sys = p["messages"][0]["content"].as_str().unwrap();
        assert!(sys.starts_with("基础提示"), "{sys}");
        assert!(sys.contains("[当前角色：架构师]"), "{sys}");
        assert!(sys.ends_with("你是资深架构师"), "{sys}");
        let _ = std::fs::remove_file(temp_file("a.json"));
    }

    #[test]
    fn no_role_no_injection() {
        let path = temp_file("b.json");
        let _ = std::fs::remove_file(&path);
        // 未激活
        std::fs::write(
            &path,
            r#"{"active":null,"roles":[{"id":"x","name":"X","prompt":"p"}]}"#,
        )
        .unwrap();
        let inj = RoleInjector::new(path.clone());
        let mut p = payload(serde_json::json!([{ "role": "system", "content": "基础提示" }]));
        inj.inject(&mut p);
        assert_eq!(p["messages"][0]["content"], "基础提示");
        // 文件缺失 → 不改 payload
        let _ = std::fs::remove_file(&path);
        let inj2 = RoleInjector::new(path);
        let mut p2 = payload(serde_json::json!([{ "role": "system", "content": "基础提示" }]));
        inj2.inject(&mut p2);
        assert_eq!(p2["messages"][0]["content"], "基础提示");
        let _ = std::fs::remove_file(temp_file("b.json"));
    }

    #[test]
    fn inserts_system_message_when_absent() {
        let path = temp_file("c.json");
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            r#"{"active":"r1","roles":[{"id":"r1","name":"R1","prompt":"角色提示"}]}"#,
        )
        .unwrap();
        let inj = RoleInjector::new(path);
        let mut p = payload(serde_json::json!([{ "role": "user", "content": "hi" }]));
        inj.inject(&mut p);
        assert_eq!(p["messages"][0]["role"], "system");
        assert!(p["messages"][0]["content"].as_str().unwrap().contains("角色提示"));
        assert_eq!(p["messages"][1]["role"], "user");
        let _ = std::fs::remove_file(temp_file("c.json"));
    }
}
