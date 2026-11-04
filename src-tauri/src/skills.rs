use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSkill {
    pub name: String,
    pub description: String,
}

/// Slash-command token. Rejects paths, flags, and whitespace.
pub fn is_skill_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() || name.len() > 64 {
        return false;
    }
    name.chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

pub fn project_has_skill(cwd: &Path, name: &str) -> bool {
    is_skill_name(name) && list_project_skills(cwd).iter().any(|skill| skill.name == name)
}

pub fn compose_prompt(prompt: &str, skill: Option<&str>) -> String {
    match skill {
        Some(name) if prompt.is_empty() => format!("/{name}"),
        Some(name) => format!("/{name}\n{prompt}"),
        None => prompt.to_string(),
    }
}

/// Skills shipped in `<cwd>/.claude/skills/<name>/SKILL.md`. Names that leave the project are skipped.
pub fn list_project_skills(cwd: &Path) -> Vec<ProjectSkill> {
    let skills_root = cwd.join(".claude").join("skills");
    let Ok(root) = skills_root.canonicalize() else {
        return Vec::new();
    };
    let Ok(cwd_real) = cwd.canonicalize() else {
        return Vec::new();
    };
    if !root.starts_with(&cwd_real) {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(folder) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let file = path.join("SKILL.md");
        if !file.is_file() {
            continue;
        }
        let Ok(real_file) = file.canonicalize() else {
            continue;
        };
        if !real_file.starts_with(&root) {
            continue;
        }
        let Ok(raw) = fs::read(&real_file) else {
            continue;
        };
        let text = String::from_utf8_lossy(&raw[..raw.len().min(8192)]);
        if let Some(skill) = skill_from_markdown(folder, &text) {
            out.push(skill);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.truncate(80);
    out
}

pub fn skill_from_markdown(folder: &str, text: &str) -> Option<ProjectSkill> {
    let (named, description) = frontmatter(text).unwrap_or((None, None));
    let name = named.filter(|value| is_skill_name(value)).or_else(|| {
        is_skill_name(folder).then(|| folder.to_string())
    })?;
    Some(ProjectSkill {
        name,
        description: description.unwrap_or_default(),
    })
}

fn frontmatter(text: &str) -> Option<(Option<String>, Option<String>)> {
    let text = text.trim_start_matches('\u{feff}');
    let rest = text.strip_prefix("---")?;
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))
        .unwrap_or(rest);
    let end = rest.find("\r\n---").or_else(|| rest.find("\n---"))?;
    let mut name = None;
    let mut description = None;
    for line in rest[..end].lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(clean_meta(value));
        } else if let Some(value) = line.strip_prefix("description:") {
            description = Some(clean_meta(value));
        }
    }
    Some((name, description))
}

fn clean_meta(value: &str) -> String {
    let unquoted = unquote(value.trim());
    let collapsed = unquoted.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.contains("sk-ant") {
        return String::new();
    }
    collapsed.chars().take(160).collect()
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let open = bytes[0];
        let close = bytes[bytes.len() - 1];
        if (open == b'"' && close == b'"') || (open == b'\'' && close == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_frontmatter_and_skips_unsafe_names() {
        let root = std::env::temp_dir().join(format!(
            "useup-skills-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let good = root.join(".claude").join("skills").join("review");
        fs::create_dir_all(&good).unwrap();
        fs::write(
            good.join("SKILL.md"),
            "---\r\nname: review\r\ndescription: \"Look at the diff\"\r\n---\r\n# body\r\n",
        )
        .unwrap();
        let bad = root.join(".claude").join("skills").join("not a skill");
        fs::create_dir_all(&bad).unwrap();
        fs::write(
            bad.join("SKILL.md"),
            "---\nname: ../secret\ndescription: sk-ant-example token\n---\n",
        )
        .unwrap();
        let listed = list_project_skills(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "review");
        assert_eq!(listed[0].description, "Look at the diff");
        assert!(!is_skill_name("../secret"));
        assert!(!is_skill_name("-flag"));
        assert_eq!(compose_prompt("check this", Some("review")), "/review\ncheck this");
        assert_eq!(compose_prompt("", Some("review")), "/review");
    }

    #[test]
    fn folder_name_is_used_when_frontmatter_has_no_name() {
        let skill = skill_from_markdown("lint", "# just a heading\n").unwrap();
        assert_eq!(skill.name, "lint");
        assert_eq!(skill.description, "");
        assert!(skill_from_markdown("bad name", "# x").is_none());
    }
}
