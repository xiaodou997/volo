//! Skill Runtime
//! 技能 = 含 SKILL.md 的目录，向内置 Agent 提供可复用的任务指令。
//! 目录：`app_data_dir/skills/<name>/SKILL.md`
//!
//! SKILL.md 格式：YAML frontmatter（name 必填，description/version 可选）+ Markdown 正文。
//! Agent system prompt 只列 name + description（渐进披露），模型判断匹配时
//! 经 skill_load 工具加载正文，避免无关指令污染上下文。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::error::{Result, VoloError};
use crate::plugin::manager::copy_dir_all;

/// 技能元数据（frontmatter 解析结果，camelCase 给前端）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub version: String,
}

/// 技能目录：`app_data_dir/skills/`
pub fn skills_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| VoloError::Other(format!("app_data_dir unavailable: {}", e)))?
        .join("skills"))
}

/// 解析 SKILL.md：frontmatter + 正文，返回 (meta, body)
fn parse_skill_md(content: &str) -> Result<(SkillMeta, String)> {
    let content = content.trim_start_matches('\u{feff}');
    let rest = content
        .strip_prefix("---")
        .ok_or_else(|| VoloError::Other("SKILL.md 缺少 frontmatter（--- 起始）".to_string()))?;
    let end = rest
        .find("\n---")
        .ok_or_else(|| VoloError::Other("SKILL.md frontmatter 未闭合（缺少 --- 结尾）".to_string()))?;
    let front = &rest[..end];
    let body = rest[end + 4..].trim_start().to_string();

    let mut name = String::new();
    let mut description = String::new();
    let mut version = String::new();
    // 极简 frontmatter 解析：只认顶行 key: value（不引入 yaml 依赖）
    for line in front.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key.trim() {
                "name" => name = value.to_string(),
                "description" => description = value.to_string(),
                "version" => version = value.to_string(),
                _ => {}
            }
        }
    }
    if name.is_empty() {
        return Err(VoloError::Other("SKILL.md frontmatter 缺少 name".to_string()));
    }
    Ok((SkillMeta { name, description, version }, body))
}

/// 读取单个技能目录（dir/SKILL.md），无效则返回 None
fn read_skill(dir: &Path) -> Option<(SkillMeta, String)> {
    let content = fs::read_to_string(dir.join("SKILL.md")).ok()?;
    match parse_skill_md(&content) {
        Ok(parsed) => Some(parsed),
        Err(e) => {
            tracing::warn!("skip invalid skill {:?}: {}", dir, e);
            None
        }
    }
}

/// 扫描技能目录下的全部技能（按 name 排序；坏目录跳过）
pub fn scan_skills(dir: &Path) -> Vec<SkillMeta> {
    let mut metas = Vec::new();
    if dir.is_dir() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some((meta, _)) = read_skill(&entry.path()) {
                        metas.push(meta);
                    }
                }
            }
        }
    }
    metas.sort_by(|a, b| a.name.cmp(&b.name));
    metas
}

/// 按 name 加载技能正文（skill_load 工具用）。逐个扫描匹配，天然防目录穿越
pub fn load_skill_body(dir: &Path, name: &str) -> Result<String> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)?.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some((meta, body)) = read_skill(&path) {
                if meta.name == name {
                    return Ok(body);
                }
            }
        }
    }
    Err(VoloError::NotFound(format!("skill: {}", name)))
}

/// 安装：校验源目录含合法 SKILL.md 后，按 frontmatter name 拷贝进技能目录（同名覆盖）
fn install_from_dir(skills_dir: &Path, source: &Path) -> Result<SkillMeta> {
    let (meta, _) = read_skill(source).ok_or_else(|| {
        VoloError::Other(format!("SKILL.md not found or invalid in {:?}", source))
    })?;
    let target = skills_dir.join(&meta.name);
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    copy_dir_all(&source.to_path_buf(), &target)?;
    Ok(meta)
}

/// 卸载：按 name 找到所属目录删除（容忍手动放入的异名目录）
fn remove_skill(skills_dir: &Path, name: &str) -> Result<()> {
    if skills_dir.is_dir() {
        for entry in fs::read_dir(skills_dir)?.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some((meta, _)) = read_skill(&path) {
                if meta.name == name {
                    fs::remove_dir_all(&path)?;
                    return Ok(());
                }
            }
        }
    }
    Err(VoloError::NotFound(format!("skill: {}", name)))
}

// ============ 内置技能播种 ============

/// 定位打包的内置技能目录（生产：resource_dir/skills；dev：仓库根 skills/）
fn builtin_skills_dir(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(dir) = app.path().resource_dir() {
        let bundled = dir.join("skills");
        if bundled.exists() {
            return Some(bundled);
        }
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../skills");
    if dev.exists() {
        Some(dev)
    } else {
        None
    }
}

/// 判断内置技能是否需要（重新）播种：
/// 目标目录不存在、已安装副本损坏、或已安装版本与内置版本不一致时返回 true
fn should_reseed_skill(target: &Path, bundled_version: &str) -> bool {
    if !target.exists() {
        return true;
    }
    match read_skill(target) {
        Some((installed, _)) => installed.version != bundled_version,
        None => true,
    }
}

/// 把 source 下的内置技能播种到技能目录（版本一致则跳过）。失败只告警不阻断
fn seed_from_dir(source: &Path, skills_dir: &Path) {
    let entries = match fs::read_dir(source) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("Failed to read builtin skills dir {:?}: {}", source, e);
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some((meta, _)) = read_skill(&path) else {
            continue;
        };
        let target = skills_dir.join(&meta.name);
        if !should_reseed_skill(&target, &meta.version) {
            continue;
        }
        if target.exists() {
            if let Err(e) = fs::remove_dir_all(&target) {
                tracing::warn!("Failed to remove outdated builtin skill {}: {}", meta.name, e);
                continue;
            }
        }
        match copy_dir_all(&path, &target) {
            Ok(()) => tracing::info!("Seeded builtin skill: {} (v{})", meta.name, meta.version),
            Err(e) => tracing::warn!("Failed to seed builtin skill {}: {}", meta.name, e),
        }
    }
}

/// 启动时播种内置技能（参照插件播种：已安装且版本一致的跳过，版本变化时覆盖更新）
pub fn seed_builtin_skills(app: &AppHandle) {
    let Some(source) = builtin_skills_dir(app) else {
        return;
    };
    let Ok(dir) = skills_dir(app) else {
        return;
    };
    if let Err(e) = fs::create_dir_all(&dir) {
        tracing::warn!("Failed to create skills dir {:?}: {}", dir, e);
        return;
    }
    seed_from_dir(&source, &dir);
}

// ============ Tauri Commands ============

/// 列出已安装技能（设置页用）
#[tauri::command]
pub fn skill_list(app: AppHandle) -> Result<Vec<SkillMeta>> {
    Ok(scan_skills(&skills_dir(&app)?))
}

/// 从本地目录安装技能（拷贝进技能目录，同名覆盖）
#[tauri::command]
pub fn skill_install_from_dir(app: AppHandle, source_dir: String) -> Result<SkillMeta> {
    let dir = skills_dir(&app)?;
    fs::create_dir_all(&dir)?;
    install_from_dir(&dir, Path::new(&source_dir))
}

/// 按 name 卸载技能
#[tauri::command]
pub fn skill_remove(app: AppHandle, name: String) -> Result<()> {
    remove_skill(&skills_dir(&app)?, &name)
}

/// 打开技能目录（便于手动管理；目录不存在则先创建）
#[tauri::command]
pub fn open_skills_dir(app: AppHandle) -> Result<()> {
    let dir = skills_dir(&app)?;
    fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<String>)
        .map_err(|e| VoloError::Other(format!("open skills dir failed: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_skills_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "volo-skill-test-{}-{}",
            tag,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_skill(dir: &Path, sub: &str, content: &str) {
        let skill_dir = dir.join(sub);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    const VALID: &str = "---\nname: weekly-report\ndescription: 生成结构化周报\nversion: 1.0.0\n---\n\n按以下结构输出周报：……\n";

    #[test]
    fn test_parse_skill_md_full() {
        let (meta, body) = parse_skill_md(VALID).unwrap();
        assert_eq!(meta.name, "weekly-report");
        assert_eq!(meta.description, "生成结构化周报");
        assert_eq!(meta.version, "1.0.0");
        assert!(body.starts_with("按以下结构输出周报"));
    }

    #[test]
    fn test_parse_skill_md_minimal_and_quotes() {
        let (meta, _) = parse_skill_md("---\nname: \"demo-skill\"\n---\n正文").unwrap();
        assert_eq!(meta.name, "demo-skill");
        assert_eq!(meta.description, "");
        assert_eq!(meta.version, "");
    }

    #[test]
    fn test_parse_skill_md_rejects_bad() {
        assert!(parse_skill_md("没有 frontmatter").is_err());
        assert!(parse_skill_md("---\nname: a\n没有结尾").is_err());
        assert!(parse_skill_md("---\ndescription: 缺 name\n---\nx").is_err());
    }

    #[test]
    fn test_scan_and_load() {
        let dir = temp_skills_dir("scan");
        write_skill(&dir, "weekly-report", VALID);
        write_skill(&dir, "b-skill", "---\nname: alpha\ndescription: 排序在前\n---\nx");
        // 坏目录（无 SKILL.md / 非法 frontmatter）跳过
        fs::create_dir_all(dir.join("empty-dir")).unwrap();
        write_skill(&dir, "bad", "no frontmatter");
        // 散文件忽略
        fs::write(dir.join("loose.md"), VALID).unwrap();

        let metas = scan_skills(&dir);
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].name, "alpha"); // 按 name 排序
        assert_eq!(metas[1].name, "weekly-report");

        let body = load_skill_body(&dir, "weekly-report").unwrap();
        assert!(body.contains("周报"));
        assert!(load_skill_body(&dir, "not-exist").is_err());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_install_and_remove() {
        let skills = temp_skills_dir("install-target");
        let source = temp_skills_dir("install-source");
        write_skill(&source, "my-skill", VALID);

        // 安装：按 frontmatter name 落目录
        let meta = install_from_dir(&skills, &source.join("my-skill")).unwrap();
        assert_eq!(meta.name, "weekly-report");
        assert!(skills.join("weekly-report/SKILL.md").exists());

        // 同名覆盖：改内容重装
        write_skill(&source, "my-skill", "---\nname: weekly-report\ndescription: v2\n---\n新正文");
        install_from_dir(&skills, &source.join("my-skill")).unwrap();
        assert_eq!(scan_skills(&skills).len(), 1);
        assert_eq!(load_skill_body(&skills, "weekly-report").unwrap(), "新正文");

        // 卸载
        remove_skill(&skills, "weekly-report").unwrap();
        assert!(scan_skills(&skills).is_empty());
        assert!(remove_skill(&skills, "weekly-report").is_err());

        // 坏源目录报错
        assert!(install_from_dir(&skills, &source.join("missing")).is_err());

        fs::remove_dir_all(&skills).ok();
        fs::remove_dir_all(&source).ok();
    }

    #[test]
    fn test_seed_from_dir() {
        let bundled = temp_skills_dir("seed-source");
        let skills = temp_skills_dir("seed-target");
        write_skill(&bundled, "weekly-report", VALID);

        // 首次播种
        seed_from_dir(&bundled, &skills);
        assert_eq!(scan_skills(&skills).len(), 1);

        // 版本一致：跳过（手动改过的正文不被覆盖）
        fs::write(
            skills.join("weekly-report/SKILL.md"),
            "---\nname: weekly-report\nversion: 1.0.0\n---\n用户改过的正文",
        )
        .unwrap();
        seed_from_dir(&bundled, &skills);
        assert_eq!(
            load_skill_body(&skills, "weekly-report").unwrap(),
            "用户改过的正文"
        );

        // 内置版本升级：覆盖重播
        write_skill(
            &bundled,
            "weekly-report",
            "---\nname: weekly-report\nversion: 1.1.0\n---\n新版正文",
        );
        seed_from_dir(&bundled, &skills);
        assert_eq!(
            load_skill_body(&skills, "weekly-report").unwrap(),
            "新版正文"
        );

        // 已安装副本损坏：重播修复
        fs::write(skills.join("weekly-report/SKILL.md"), "损坏").unwrap();
        seed_from_dir(&bundled, &skills);
        assert_eq!(
            load_skill_body(&skills, "weekly-report").unwrap(),
            "新版正文"
        );

        // 坏的内置目录跳过、散文件忽略
        write_skill(&bundled, "bad", "no frontmatter");
        fs::write(bundled.join("loose.md"), VALID).unwrap();
        seed_from_dir(&bundled, &skills);
        assert_eq!(scan_skills(&skills).len(), 1);

        fs::remove_dir_all(&bundled).ok();
        fs::remove_dir_all(&skills).ok();
    }
}
