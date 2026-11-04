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

pub fn has_skill(cwd: &Path, name: &str) -> bool {
    is_skill_name(name) && list_available_skills(cwd).iter().any(|skill| skill.name == name)
}

pub fn compose_prompt(prompt: &str, skill: Option<&str>) -> String {
    match skill {
        Some(name) if prompt.is_empty() => format!("/{name}"),
        Some(name) => format!("/{name}\n{prompt}"),
        None => prompt.to_string(),
    }
}

const MAX_SKILLS: usize = 300;

/// Project skills, then the user skills folder, then skills from plugins Claude has installed and not disabled.
/// The same slash name is kept once. A project skill wins over a user skill, which wins over a plugin skill.
pub fn list_available_skills(cwd: &Path) -> Vec<ProjectSkill> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    if let Some(root) = contained_dir(cwd, &cwd.join(".claude").join("skills")) {
        collect_tree(&root, &mut out, &mut seen);
    }
    if let Some(home) = dirs::home_dir() {
        collect_tree(&home.join(".claude").join("skills"), &mut out, &mut seen);
        for dir in plugin_skill_dirs(&home, cwd) {
            collect_tree(&dir, &mut out, &mut seen);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.truncate(MAX_SKILLS);
    out
}

fn contained_dir(parent: &Path, child: &Path) -> Option<std::path::PathBuf> {
    let parent = parent.canonicalize().ok()?;
    let child = child.canonicalize().ok()?;
    child.starts_with(&parent).then_some(child)
}

fn collect_tree(root: &Path, out: &mut Vec<ProjectSkill>, seen: &mut std::collections::BTreeSet<String>) {
    let Ok(root_real) = root.canonicalize() else {
        return;
    };
    let mut stack = vec![(root_real.clone(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 5 || out.len() >= MAX_SKILLS {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let Ok(real) = path.canonicalize() else {
                continue;
            };
            if !real.starts_with(&root_real) {
                continue;
            }
            if kind.is_file() && entry.file_name() == "SKILL.md" && depth > 0 {
                let Some(folder) = path.parent().and_then(|parent| parent.file_name()).and_then(|name| name.to_str()) else {
                    continue;
                };
                let Ok(raw) = fs::read(&real) else {
                    continue;
                };
                let text = String::from_utf8_lossy(&raw[..raw.len().min(8192)]);
                if let Some(skill) = skill_from_markdown(folder, &text) {
                    if seen.insert(skill.name.clone()) {
                        out.push(skill);
                    }
                }
            } else if kind.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name == "node_modules" || name == ".git" || name == "target" {
                    continue;
                }
                stack.push((real, depth + 1));
            }
        }
    }
}

fn plugin_skill_dirs(home: &Path, cwd: &Path) -> Vec<std::path::PathBuf> {
    let mut enabled = std::collections::BTreeMap::new();
    read_enabled(&home.join(".claude").join("settings.json"), &mut enabled);
    read_enabled(&cwd.join(".claude").join("settings.json"), &mut enabled);
    read_enabled(&cwd.join(".claude").join("settings.local.json"), &mut enabled);
    let mut dirs = Vec::new();
    for manifest in [
        home.join(".claude").join("plugins").join("installed_plugins.json"),
        cwd.join(".claude").join("plugins").join("installed_plugins.json"),
    ] {
        for (id, install) in read_installs(&manifest) {
            if enabled.get(&id).copied().unwrap_or(true) {
                dirs.push(install.join("skills"));
            }
        }
    }
    dirs
}

fn read_enabled(path: &Path, into: &mut std::collections::BTreeMap<String, bool>) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let Some(map) = value.get("enabledPlugins").and_then(|item| item.as_object()) else {
        return;
    };
    for (id, flag) in map {
        if let Some(flag) = flag.as_bool() {
            into.insert(id.clone(), flag);
        }
    }
}

fn read_installs(path: &Path) -> Vec<(String, std::path::PathBuf)> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    installs_from_json(&raw)
}

fn installs_from_json(raw: &str) -> Vec<(String, std::path::PathBuf)> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(plugins) = value.get("plugins").and_then(|item| item.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (id, records) in plugins {
        let Some(records) = records.as_array() else {
            continue;
        };
        for record in records {
            if let Some(path) = record.get("installPath").and_then(|item| item.as_str()) {
                let path = path.trim();
                if !path.is_empty() {
                    out.push((id.clone(), std::path::PathBuf::from(path)));
                }
            }
        }
    }
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
        let mut listed = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        collect_tree(&root.join(".claude").join("skills"), &mut listed, &mut seen);
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

    #[test]
    fn project_skill_hides_a_later_duplicate() {
        let root = std::env::temp_dir().join(format!(
            "useup-skill-merge-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("project").join(".claude").join("skills").join("review");
        let user = root.join("user").join("pdf");
        let again = root.join("user").join("review");
        let plugin = root.join("plugin").join("setup-pstack");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&user).unwrap();
        fs::create_dir_all(&again).unwrap();
        fs::create_dir_all(&plugin).unwrap();
        fs::write(project.join("SKILL.md"), "---\nname: review\ndescription: From the project\n---\n").unwrap();
        fs::write(user.join("SKILL.md"), "---\nname: pdf\ndescription: Read a pdf\n---\n").unwrap();
        fs::write(again.join("SKILL.md"), "---\nname: review\ndescription: From the user\n---\n").unwrap();
        fs::write(plugin.join("SKILL.md"), "---\nname: setup-pstack\ndescription: Configure models\n---\n").unwrap();

        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        collect_tree(&root.join("project").join(".claude").join("skills"), &mut out, &mut seen);
        collect_tree(&root.join("user"), &mut out, &mut seen);
        collect_tree(&root.join("plugin").parent().unwrap(), &mut out, &mut seen);
        let _ = fs::remove_dir_all(&root);

        let review = out.iter().find(|skill| skill.name == "review").unwrap();
        assert_eq!(review.description, "From the project");
        assert_eq!(out.iter().filter(|skill| skill.name == "review").count(), 1);
        assert!(out.iter().any(|skill| skill.name == "pdf"));
        assert!(out.iter().any(|skill| skill.name == "setup-pstack"));
    }

    #[test]
    fn disabled_plugin_is_not_a_skill_dir() {
        let raw = r#"{"plugins":{"pstack@cursor-plugins":[{"installPath":"C:/plugins/pstack"}],"other@market":[{"installPath":"C:/plugins/other"}]}}"#;
        let installs = installs_from_json(raw);
        let mut enabled: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
        enabled.insert("other@market".to_string(), false);
        let dirs: Vec<_> = installs
            .into_iter()
            .filter(|(id, _)| enabled.get(id).copied().unwrap_or(true))
            .map(|(_, path)| path.join("skills"))
            .collect();
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].ends_with("skills"));
        assert!(dirs[0].components().any(|part| part.as_os_str() == "pstack"));
    }
}
