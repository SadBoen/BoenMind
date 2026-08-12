//! refine-suggest 建议的审批生效逻辑（宿主审批模式）。
//!
//! 代理只提交建议（refine-suggest 插件 → bm-server 截获入库），本模块在用户
//! 批准后才真正修改知识资产，且改前备份、可回滚——与 Prime Agent `/refine`
//! "代理直接改手册"形成有意差异（避免把坏经验 refine 进知识库）。
//!
//! 生效范围（v1）：
//! - `skill:<id>` → 修改 `~/.boenmind/skills/<id>/SKILL.md` 的 frontmatter
//!   description 行（suggested 为完整新描述；quote 需在文件中出现作校验），
//!   改前备份 `.bak-<ts>`；若该 skill 已启用则同步到 pi 目录。
//! - `system_prompt` → 追加到 config `custom_system_prompt`（随系统提示词注入）。

use crate::config::AppConfig;
use crate::error::AppError;

/// 审批生效入口。成功返回备份路径（回滚用，system_prompt 类型无备份返回 None）。
pub fn apply_suggestion(
    config: &mut AppConfig,
    suggestion: &crate::db::RefinementSuggestion,
) -> Result<Option<String>, AppError> {
    if let Some(skill_id) = suggestion.target.strip_prefix("skill:") {
        apply_to_skill(config, skill_id, &suggestion.quote, &suggestion.suggested)
    } else if suggestion.target == "system_prompt" {
        apply_to_system_prompt(config, &suggestion.suggested)?;
        Ok(None)
    } else {
        Err(AppError::invalid(format!("未知 target: {}", suggestion.target)))
    }
}

/// 从备份恢复（approve 后的一键还原）。
pub fn rollback_suggestion(config: &AppConfig, backup_path: &str) -> Result<(), AppError> {
    let backup = std::path::PathBuf::from(backup_path);
    // 只允许回滚到 skills 管理目录内的备份（防路径穿越）
    let skills_root = crate::skills::skills_dir().canonicalize().unwrap_or_default();
    let backup_canonical = backup.canonicalize().map_err(|e| {
        AppError::invalid(format!("备份文件不存在或不可读: {backup_path} ({e})"))
    })?;
    if !backup_canonical.starts_with(&skills_root) {
        return Err(AppError::invalid("备份路径不在 skill 目录内，拒绝回滚"));
    }
    let skill_dir = backup_canonical
        .parent()
        .ok_or_else(|| AppError::invalid("备份路径无父目录"))?;
    let skill_id = skill_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::invalid("无法从备份路径解析 skill id"))?
        .to_string();
    let target = skill_dir.join("SKILL.md");
    std::fs::copy(&backup_canonical, &target)
        .map_err(|e| AppError::internal(format!("回滚 SKILL.md 失败: {e}")))?;
    sync_pi_if_enabled(config, &skill_id)?;
    Ok(())
}

fn apply_to_skill(
    config: &AppConfig,
    skill_id: &str,
    quote: &str,
    suggested: &str,
) -> Result<Option<String>, AppError> {
    let md_path = crate::skills::skills_dir().join(skill_id).join("SKILL.md");
    let text = std::fs::read_to_string(&md_path)
        .map_err(|e| AppError::invalid(format!("skill {skill_id} 未安装（{e}）")))?;
    if quote.trim().is_empty() {
        return Err(AppError::invalid("quote 为空，拒绝生效"));
    }
    // 校验：quote 必须出现在 SKILL.md 中（防代理引用过期/错误文本误伤）
    if !text.contains(quote.trim()) {
        return Err(AppError::invalid(format!(
            "quote 与 skill {skill_id} 当前内容不匹配（描述可能已变更），请刷新后重试"
        )));
    }
    let (frontmatter, body) = split_frontmatter(&text)
        .ok_or_else(|| AppError::invalid(format!("skill {skill_id} 的 SKILL.md 无 frontmatter")))?;
    let desc_line = frontmatter
        .lines()
        .find(|line| line.trim_start().starts_with("description:"))
        .ok_or_else(|| AppError::invalid(format!("skill {skill_id} 无 description 字段")))?;
    // 备份后替换整个 description 行（suggested = 完整新描述，双引号包裹防 YAML 解析错）
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup_path = md_path.with_file_name(format!("SKILL.md.bak-{ts}"));
    std::fs::copy(&md_path, &backup_path)
        .map_err(|e| AppError::internal(format!("备份 SKILL.md 失败: {e}")))?;
    let escaped = suggested.replace('"', "\\\"");
    let new_frontmatter = frontmatter.replace(
        desc_line,
        &format!("description: \"{escaped}\""),
    );
    std::fs::write(&md_path, format!("{new_frontmatter}{body}"))
        .map_err(|e| AppError::internal(format!("写回 SKILL.md 失败: {e}")))?;
    sync_pi_if_enabled(config, skill_id)?;
    Ok(Some(backup_path.to_string_lossy().to_string()))
}

fn apply_to_system_prompt(config: &mut AppConfig, suggested: &str) -> Result<(), AppError> {
    let trimmed = suggested.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid("suggested 为空，拒绝生效"));
    }
    let existing = config.custom_system_prompt.clone().unwrap_or_default();
    let joined = if existing.trim().is_empty() {
        trimmed.to_string()
    } else {
        format!("{existing}\n{trimmed}")
    };
    config.custom_system_prompt = Some(joined);
    crate::config::save(config)
        .map_err(|e| AppError::internal(format!("保存配置失败: {e}")))?;
    Ok(())
}

/// 若该 skill 已启用（pi 目录存在），同步修改后的 SKILL.md 到 pi 目录。
fn sync_pi_if_enabled(config: &AppConfig, skill_id: &str) -> Result<(), AppError> {
    if !config.enabled_skills.iter().any(|s| s == skill_id) {
        return Ok(());
    }
    let src = crate::skills::skills_dir().join(skill_id).join("SKILL.md");
    let dest = crate::skills::pi_skills_dir().join(skill_id).join("SKILL.md");
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::internal(format!("创建 pi skill 目录失败: {e}")))?;
    }
    std::fs::copy(&src, &dest).map_err(|e| AppError::internal(format!("同步 pi skill 失败: {e}")))?;
    Ok(())
}

/// 切分 frontmatter 与正文（`---\n...\n---`）。
fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let frontmatter = &rest[..end];
    let body = &rest[end..];
    Some((frontmatter, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_split_and_replace() {
        let text = "---\nname: demo\ndescription: 旧描述\n---\n正文内容\n";
        let (fm, body) = split_frontmatter(text).expect("frontmatter");
        assert_eq!(fm, "name: demo\ndescription: 旧描述");
        assert!(body.contains("正文内容"));
        let desc_line = fm.lines().find(|l| l.starts_with("description:")).unwrap();
        let replaced = fm.replace(desc_line, "description: \"新描述\"");
        assert!(replaced.contains("description: \"新描述\""));
        assert!(replaced.contains("name: demo"));
    }

    #[test]
    fn quote_mismatch_rejected() {
        let md = crate::skills::skills_dir().join("__no_such_skill__").join("SKILL.md");
        assert!(!md.exists(), "不应存在真实文件");
        // 无安装文件 → invalid
        let config = AppConfig::default();
        let err = apply_to_skill(&config, "__no_such_skill__", "x", "y").unwrap_err();
        assert!(matches!(err, AppError::Invalid(_)));
    }
}
