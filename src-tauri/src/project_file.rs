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

/// Replace a text file inside the project. The session file never stores this body.
pub(crate) fn write_in_project(cwd: &Path, raw_path: &str, content: &str) -> Result<(), String> {
    if content.len() > PANEL_MAX_BYTES || content.as_bytes().contains(&0) {
        return Err("unsupported".into());
    }
    let full = resolve_inside(cwd, raw_path)?;
    if !full.is_file() || is_binary_extension(&full) {
        return Err("unsupported".into());
    }
    std::fs::write(&full, content).map_err(|_| "write failed".to_string())?;
    Ok(())
}

/// Whether `raw` is a file inside `cwd`. Does not read the body.
pub(crate) fn locate_in_project(cwd: &Path, raw_path: &str) -> Result<(), String> {
    let full = resolve_inside(cwd, raw_path)?;
    if full.is_file() {
        Ok(())
    } else {
        Err("not a file".into())
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
    // Relative paths are judged before canonicalize so `../` cannot leave the
    // folder. Absolute paths wait until both sides are canonical: on Windows
    // `canonicalize` adds `\\?\`, and a junction's stored name differs from
    // the folder the session was opened with. Either mismatch used to reject
    // a file that still lands inside the project.
    if !requested.is_absolute() && !starts_inside(&lexical, &cwd) {
        return Err("outside project".into());
    }
    let real = lexical
        .canonicalize()
        .map_err(|_| "missing file".to_string())?;
    if starts_inside(&real, &cwd) {
        Ok(real)
    } else {
        Err("outside project".into())
    }
}

fn starts_inside(path: &Path, root: &Path) -> bool {
    let path = strip_verbatim(path);
    let root = strip_verbatim(root);
    path.starts_with(&root) && path != root
}

/// `\\?\C:\proj` and `C:\proj` are the same folder. UNC verbatim prefixes too.
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest.to_string());
    }
    path.to_path_buf()
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
        assert!(locate_in_project(&root, "nested").is_err());
        assert!(locate_in_project(&root, "notes.md").is_ok());

        let _ = fs::remove_dir_all(&root);
    }

    fn display_path(path: &Path) -> PathBuf {
        let text = path.to_string_lossy();
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
        PathBuf::from(text)
    }

    #[test]
    fn absolute_display_path_reads_when_cwd_is_not_verbatim() {
        let root = scratch();
        fs::write(root.join("STATUS.md"), "status").unwrap();
        let outside = scratch();
        fs::write(outside.join("secret.txt"), "nope").unwrap();
        let cwd = display_path(&root);
        let file = cwd.join("STATUS.md");
        let raw = file.to_str().unwrap();
        assert!(Path::new(raw).is_absolute());
        if root.to_string_lossy().starts_with(r"\\?\") {
            assert_ne!(cwd, root);
        }

        let view = read_in_project(&cwd, raw).unwrap();
        assert_eq!(view.kind, ProjectFileKind::Markdown);
        assert_eq!(view.content.as_deref(), Some("status"));
        let relative = read_in_project(&cwd, "STATUS.md").unwrap();
        assert_eq!(relative.content.as_deref(), Some("status"));

        let secret = display_path(&outside).join("secret.txt");
        assert!(read_in_project(&cwd, secret.to_str().unwrap()).is_err());
        assert!(locate_in_project(&cwd, secret.to_str().unwrap()).is_err());
        let sibling = outside.file_name().unwrap().to_string_lossy();
        let climbed = format!("../{sibling}/secret.txt");
        assert!(read_in_project(&root, &climbed).is_err());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn text_edit_roundtrips_inside_the_project_only() {
        let root = scratch();
        fs::write(root.join("notes.md"), "# Old\n").unwrap();
        fs::write(root.join("plain.txt"), "old").unwrap();
        fs::write(root.join("pic.png"), [0_u8, 1, 2, 3]).unwrap();
        let outside = scratch();
        fs::write(outside.join("secret.txt"), "keep").unwrap();
        let cwd = display_path(&root);
        let notes = cwd.join("notes.md");

        write_in_project(&cwd, notes.to_str().unwrap(), "# New\n").unwrap();
        let view = read_in_project(&cwd, "notes.md").unwrap();
        assert_eq!(view.content.as_deref(), Some("# New\n"));
        write_in_project(&root, "plain.txt", "newer").unwrap();
        assert_eq!(
            read_in_project(&root, "plain.txt").unwrap().content.as_deref(),
            Some("newer")
        );

        assert!(write_in_project(&cwd, cwd.join("pic.png").to_str().unwrap(), "nope").is_err());
        assert_eq!(fs::read(root.join("pic.png")).unwrap(), vec![0, 1, 2, 3]);
        let secret = display_path(&outside).join("secret.txt");
        assert!(write_in_project(&cwd, secret.to_str().unwrap(), "pwn").is_err());
        assert_eq!(fs::read_to_string(outside.join("secret.txt")).unwrap(), "keep");
        assert!(write_in_project(&root, "../secret.txt", "no").is_err());
        assert!(write_in_project(&root, "plain.txt", "bad\0text").is_err());
        assert_eq!(
            read_in_project(&root, "plain.txt").unwrap().content.as_deref(),
            Some("newer")
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
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

    #[cfg(windows)]
    #[test]
    fn junction_name_still_reads_and_an_outside_junction_does_not() {
        let real = scratch();
        fs::write(real.join("notes.md"), "inside").unwrap();
        let link_root = scratch();
        let link = link_root.join("proj");
        junction(&link, &real);
        let via = link.join("notes.md");
        let through_alias = read_in_project(&real, via.to_str().unwrap()).unwrap();
        assert_eq!(through_alias.content.as_deref(), Some("inside"));
        let opened_as_alias = read_in_project(&link, via.to_str().unwrap()).unwrap();
        assert_eq!(opened_as_alias.content.as_deref(), Some("inside"));

        let outside = scratch();
        fs::write(outside.join("secret.txt"), "nope").unwrap();
        let leak = real.join("leak");
        junction(&leak, &outside);
        assert!(read_in_project(&real, leak.join("secret.txt").to_str().unwrap()).is_err());
        assert!(read_in_project(&link, "leak/secret.txt").is_err());

        let _ = fs::remove_dir(&leak);
        let _ = fs::remove_dir(&link);
        let _ = fs::remove_dir_all(&link_root);
        let _ = fs::remove_dir_all(&real);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(display_path(link))
            .arg(display_path(target))
            .status()
            .expect("spawn mklink");
        assert!(status.success(), "mklink /J failed");
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
