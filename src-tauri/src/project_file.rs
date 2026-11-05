use crate::models::{ProjectFileKind, ProjectFileView};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};

/// Bytes kept for a panel read. The session file never stores this body.
const PANEL_MAX_BYTES: usize = 512 * 1024;

const MARKDOWN_EXT: &[&str] = &["md", "markdown", "mdx", "mdown"];

const BINARY_EXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "ico", "bmp", "pdf", "zip", "gz", "tgz", "bz2", "xz",
    "7z", "rar", "wasm", "exe", "dll", "so", "dylib", "bin", "class", "jar", "mp3", "mp4", "mov",
    "avi", "webm", "woff", "woff2", "ttf", "otf", "eot", "sqlite", "db", "doc", "docx", "xls",
    "xlsx", "ppt", "pptx", "iso", "img", "dmg",
];

pub(crate) fn read_in_project(cwd: &Path, raw_path: &str) -> Result<ProjectFileView, String> {
    let full = resolve_inside(cwd, raw_path)?;
    if !full.is_file() {
        return Err("not a file".into());
    }
    let display = raw_path.trim().to_string();
    if is_binary_extension(&full) {
        return Ok(unsupported(display));
    }
    let (bytes, truncated) = read_capped(&full)?;
    let Some(text) = utf8_text(&bytes) else {
        return Ok(unsupported(display));
    };
    let kind = if is_markdown_extension(&full) {
        ProjectFileKind::Markdown
    } else {
        ProjectFileKind::Text
    };
    Ok(ProjectFileView {
        path: display,
        kind,
        content: Some(text),
        truncated,
    })
}

fn unsupported(path: String) -> ProjectFileView {
    ProjectFileView {
        path,
        kind: ProjectFileKind::Unsupported,
        content: None,
        truncated: false,
    }
}

fn resolve_inside(cwd: &Path, raw: &str) -> Result<PathBuf, String> {
    let cwd = cwd
        .canonicalize()
        .map_err(|_| "project folder is missing".to_string())?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(|ch| ch.is_control()) {
        return Err("bad path".into());
    }
    let requested = Path::new(trimmed);
    let joined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        cwd.join(requested)
    };
    let lexical = normalize_lexical(&joined).ok_or_else(|| "bad path".to_string())?;
    if !lexical.starts_with(&cwd) {
        return Err("outside project".into());
    }
    match lexical.canonicalize() {
        Ok(real) => {
            if real.starts_with(&cwd) {
                Ok(real)
            } else {
                Err("outside project".into())
            }
        }
        Err(_) => Err("missing file".into()),
    }
}

fn normalize_lexical(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(_) | Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                _ => return None,
            },
            Component::Normal(part) => out.push(part),
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn extension_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
}

fn is_markdown_extension(path: &Path) -> bool {
    extension_lower(path).is_some_and(|ext| MARKDOWN_EXT.contains(&ext.as_str()))
}

fn is_binary_extension(path: &Path) -> bool {
    extension_lower(path).is_some_and(|ext| BINARY_EXT.contains(&ext.as_str()))
}

fn read_capped(path: &Path) -> Result<(Vec<u8>, bool), String> {
    let mut file = File::open(path).map_err(|_| "missing file".to_string())?;
    let mut buf = vec![0_u8; PANEL_MAX_BYTES + 1];
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err("read failed".into()),
        }
    }
    let truncated = filled > PANEL_MAX_BYTES;
    buf.truncate(filled.min(PANEL_MAX_BYTES));
    Ok((buf, truncated))
}

/// Accept UTF-8 text. A trailing cut mid-character is dropped. NUL means binary.
fn utf8_text(bytes: &[u8]) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Some(text.to_string());
    }
    let mut end = bytes.len();
    let floor = end.saturating_sub(3);
    while end > floor {
        end -= 1;
        if let Ok(text) = std::str::from_utf8(&bytes[..end]) {
            return Some(text.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "useupclaude-file-panel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn markdown_and_text_render_and_other_files_do_not() {
        let root = scratch();
        fs::write(root.join("notes.md"), "# Hello\n").unwrap();
        fs::write(root.join("plain.txt"), "just text").unwrap();
        fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("pic.png"), [0_u8, 1, 2, 3]).unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("nested").join("More.MD"), "title").unwrap();

        let md = read_in_project(&root, "notes.md").unwrap();
        assert_eq!(md.kind, ProjectFileKind::Markdown);
        assert_eq!(md.content.as_deref(), Some("# Hello\n"));
        assert!(!md.truncated);

        let text = read_in_project(&root, "plain.txt").unwrap();
        assert_eq!(text.kind, ProjectFileKind::Text);
        assert_eq!(text.content.as_deref(), Some("just text"));

        let source = read_in_project(&root, "main.rs").unwrap();
        assert_eq!(source.kind, ProjectFileKind::Text);
        assert_eq!(source.content.as_deref(), Some("fn main() {}\n"));

        let image = read_in_project(&root, "pic.png").unwrap();
        assert_eq!(image.kind, ProjectFileKind::Unsupported);
        assert!(image.content.is_none());

        let nested = read_in_project(&root, "nested/More.MD").unwrap();
        assert_eq!(nested.kind, ProjectFileKind::Markdown);
        assert_eq!(nested.content.as_deref(), Some("title"));

        let abs = root.join("plain.txt");
        let from_abs = read_in_project(&root, abs.to_str().unwrap()).unwrap();
        assert_eq!(from_abs.kind, ProjectFileKind::Text);
        assert_eq!(from_abs.content.as_deref(), Some("just text"));

        assert!(read_in_project(&root, "missing.md").is_err());
        assert!(read_in_project(&root, "../secret.txt").is_err());
        assert!(read_in_project(&root, "/etc/passwd").is_err());
        assert!(read_in_project(&root, "nested").is_err());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn oversized_text_is_capped_without_storing_a_session_body() {
        let root = scratch();
        let huge = "m".repeat(PANEL_MAX_BYTES + 50);
        fs::write(root.join("big.txt"), &huge).unwrap();
        let view = read_in_project(&root, "big.txt").unwrap();
        assert_eq!(view.kind, ProjectFileKind::Text);
        assert!(view.truncated);
        let content = view.content.unwrap();
        assert!(content.len() <= PANEL_MAX_BYTES);
        assert!(content.len() < huge.len());
        assert!(!content.contains('\0'));
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_that_leaves_the_project_is_refused() {
        use std::os::unix::fs::symlink;
        let root = scratch();
        let outside = scratch();
        fs::write(outside.join("secret.txt"), "nope").unwrap();
        symlink(outside.join("secret.txt"), root.join("leak.txt")).unwrap();
        assert!(read_in_project(&root, "leak.txt").is_err());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }
}
