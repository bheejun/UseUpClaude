use crate::models::{
    ProjectDirEntry, ProjectDirListing, ProjectFileIndex, ProjectFileKind, ProjectFileRef,
    ProjectFileView, RecentProjectFile,
};
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};

/// Bytes kept for a panel read. The session file never stores this body.
const PANEL_MAX_BYTES: usize = 512 * 1024;

/// Children returned for one folder. The rest stay on disk until the next listing.
const DIR_LIST_LIMIT: usize = 400;

/// Files walked for the open-file picker and markdown Recents candidates.
const INDEX_FILE_LIMIT: usize = 4000;
const INDEX_DIR_LIMIT: usize = 500;
const RECENT_LIMIT: usize = 12;
const PREVIEW_CHARS: usize = 700;

/// Dependency trees stay on disk. All Files still lists them one folder at a time.
const SKIP_WALK: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".next",
    "vendor",
    "__pycache__",
];

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

/// One level of `raw` inside `cwd`. An empty path lists the project folder itself.
/// Symlinks and junctions that leave the folder are omitted. No file bodies.
pub(crate) fn list_in_project(cwd: &Path, raw_path: &str) -> Result<ProjectDirListing, String> {
    let root = cwd
        .canonicalize()
        .map_err(|_| "project folder is missing".to_string())?;
    let trimmed = raw_path.trim();
    let dir = if trimmed.is_empty() || trimmed == "." {
        root.clone()
    } else {
        let full = resolve_inside(cwd, trimmed)?;
        if !full.is_dir() {
            return Err("not a directory".into());
        }
        full
    };
    let rel = relative_to(&root, &dir)?;
    let mut entries = Vec::new();
    let reader = std::fs::read_dir(&dir).map_err(|_| "read failed".to_string())?;
    for item in reader {
        let item = item.map_err(|_| "read failed".to_string())?;
        let Some(name) = item.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains(['/', '\\'])
            || name.chars().any(|ch| ch.is_control())
        {
            continue;
        }
        let Ok(real) = item.path().canonicalize() else {
            continue;
        };
        if !starts_inside(&real, &root) {
            continue;
        }
        let path = relative_to(&root, &real)?;
        entries.push(ProjectDirEntry {
            name,
            path,
            is_dir: real.is_dir(),
            modified_ms: modified_ms(&real),
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    let truncated = entries.len() > DIR_LIST_LIMIT;
    entries.truncate(DIR_LIST_LIMIT);
    Ok(ProjectDirListing {
        path: rel,
        entries,
        truncated,
    })
}

/// Every file under the project, except dependency directories. No file bodies.
pub(crate) fn index_in_project(cwd: &Path) -> Result<ProjectFileIndex, String> {
    let root = cwd
        .canonicalize()
        .map_err(|_| "project folder is missing".to_string())?;
    let mut files = Vec::new();
    let mut truncated = false;
    let mut dirs_left = INDEX_DIR_LIMIT;
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        if dirs_left == 0 || files.len() >= INDEX_FILE_LIMIT {
            truncated = true;
            break;
        }
        dirs_left -= 1;
        let Ok(reader) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in reader {
            let Ok(item) = item else {
                continue;
            };
            let Some(name) = item.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name.is_empty()
                || name == "."
                || name == ".."
                || name.contains(['/', '\\'])
                || name.chars().any(|ch| ch.is_control())
            {
                continue;
            }
            let Ok(real) = item.path().canonicalize() else {
                continue;
            };
            if !starts_inside(&real, &root) {
                continue;
            }
            let Ok(path) = relative_to(&root, &real) else {
                continue;
            };
            if real.is_dir() {
                if SKIP_WALK.iter().any(|skip| *skip == name) {
                    continue;
                }
                stack.push(real);
            } else if real.is_file() {
                if files.len() >= INDEX_FILE_LIMIT {
                    truncated = true;
                    continue;
                }
                files.push(ProjectFileRef {
                    name,
                    path,
                    modified_ms: modified_ms(&real),
                });
            }
        }
    }
    files.sort_by(|left, right| left.path.to_lowercase().cmp(&right.path.to_lowercase()));
    Ok(ProjectFileIndex { files, truncated })
}

/// Newest previewable documents for the Recents cards.
/// Matches Cursor's library Recents: markdown (and .mdx/.markdown/.mdown) only.
/// Binaries, images, folders, and other non-previewable paths stay out.
/// Snippets are not stored on the session. All Files still lists every path.
pub(crate) fn recent_in_project(cwd: &Path) -> Result<Vec<RecentProjectFile>, String> {
    let mut files = index_in_project(cwd)?.files;
    files.retain(|file| {
        resolve_inside(cwd, &file.path)
            .ok()
            .is_some_and(|path| is_markdown_extension(&path))
    });
    files.sort_by(|left, right| {
        right
            .modified_ms
            .cmp(&left.modified_ms)
            .then_with(|| left.path.to_lowercase().cmp(&right.path.to_lowercase()))
    });
    files.truncate(RECENT_LIMIT);
    let mut recent = Vec::with_capacity(files.len());
    for file in files {
        let Ok(path) = resolve_inside(cwd, &file.path) else {
            continue;
        };
        let (kind, preview) = read_preview(&path);
        // Opening or touching a non-previewable file must not land here.
        if kind != ProjectFileKind::Markdown {
            continue;
        }
        recent.push(RecentProjectFile {
            name: file.name,
            path: file.path,
            modified_ms: file.modified_ms,
            kind,
            preview,
        });
    }
    Ok(recent)
}

fn modified_ms(path: &Path) -> u64 {
    let Ok(meta) = path.metadata() else {
        return 0;
    };
    let Ok(modified) = meta.modified() else {
        return 0;
    };
    let Ok(since) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return 0;
    };
    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
}

fn read_preview(path: &Path) -> (ProjectFileKind, String) {
    if is_binary_extension(path) {
        return (ProjectFileKind::Unsupported, String::new());
    }
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return (ProjectFileKind::Unsupported, String::new()),
    };
    let mut buf = vec![0_u8; 4096];
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return (ProjectFileKind::Unsupported, String::new()),
        }
    }
    buf.truncate(filled);
    let Some(text) = utf8_text(&buf) else {
        return (ProjectFileKind::Unsupported, String::new());
    };
    let kind = if is_markdown_extension(path) {
        ProjectFileKind::Markdown
    } else {
        ProjectFileKind::Text
    };
    (kind, text.chars().take(PREVIEW_CHARS).collect())
}

fn relative_to(root: &Path, path: &Path) -> Result<String, String> {
    let root = strip_verbatim(root);
    let path = strip_verbatim(path);
    if path == root {
        return Ok(String::new());
    }
    let rel = path
        .strip_prefix(&root)
        .map_err(|_| "outside project".to_string())?;
    let mut parts = Vec::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => return Err("outside project".into()),
        }
    }
    if parts.is_empty() {
        Err("outside project".into())
    } else {
        Ok(parts.join("/"))
    }
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

    #[test]
    fn list_is_one_level_inside_the_project_and_has_no_body() {
        let root = scratch();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src").join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("README.md"), "hi").unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();
        fs::create_dir(root.join("empty")).unwrap();

        let top = list_in_project(&root, "").unwrap();
        assert_eq!(top.path, "");
        assert!(!top.truncated);
        let names: Vec<_> = top.entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["empty", "src", "b.txt", "README.md"]);
        assert!(top.entries.iter().take(2).all(|entry| entry.is_dir));
        assert!(top.entries.iter().skip(2).all(|entry| !entry.is_dir));
        assert!(top.entries.iter().all(|entry| entry.name != "main.rs"));
        let readme = top
            .entries
            .iter()
            .find(|entry| entry.name == "README.md")
            .unwrap();
        assert_eq!(readme.path, "README.md");
        assert!(readme.modified_ms > 0);
        assert_eq!(
            read_in_project(&root, &readme.path).unwrap().content.as_deref(),
            Some("hi")
        );

        let nested = list_in_project(&root, "src").unwrap();
        assert_eq!(nested.path, "src");
        assert_eq!(nested.entries.len(), 1);
        assert_eq!(nested.entries[0].name, "main.rs");
        assert_eq!(nested.entries[0].path, "src/main.rs");
        assert!(!nested.entries[0].is_dir);
        let empty = list_in_project(&root, "empty").unwrap();
        assert!(empty.entries.is_empty());
        assert!(!empty.truncated);

        assert!(list_in_project(&root, "README.md").is_err());
        assert!(list_in_project(&root, "../").is_err());
        let outside = scratch();
        fs::write(outside.join("secret.txt"), "nope").unwrap();
        assert!(list_in_project(&root, outside.to_str().unwrap()).is_err());
        assert!(list_in_project(&root, ".").unwrap().entries.len() == top.entries.len());

        let many = scratch();
        for index in 0..(DIR_LIST_LIMIT + 1) {
            fs::write(many.join(format!("f{index:04}.txt")), "x").unwrap();
        }
        let capped = list_in_project(&many, "").unwrap();
        assert!(capped.truncated);
        assert_eq!(capped.entries.len(), DIR_LIST_LIMIT);
        assert!(capped.entries.iter().all(|entry| entry.path.ends_with(".txt")));

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        let _ = fs::remove_dir_all(&many);
    }

    #[test]
    fn recents_only_include_previewable_markdown_documents() {
        let root = scratch();
        fs::create_dir(root.join("docs")).unwrap();
        fs::write(root.join("docs").join("notes.md"), "# Hello\n\nbody").unwrap();
        fs::write(root.join("guide.markdown"), "title").unwrap();
        fs::write(root.join("plain.txt"), "just text").unwrap();
        fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("pic.png"), [0_u8, 1, 2, 3]).unwrap();
        fs::create_dir(root.join("node_modules")).unwrap();
        fs::write(root.join("node_modules").join("pkg.js"), "ignore").unwrap();
        fs::create_dir(root.join("empty-dir")).unwrap();

        let recent = recent_in_project(&root).unwrap();
        assert!(recent.iter().all(|file| file.kind == ProjectFileKind::Markdown));
        let notes = recent.iter().find(|file| file.path == "docs/notes.md").unwrap();
        assert!(notes.preview.contains("# Hello"));
        assert!(notes.modified_ms > 0);
        assert!(recent.iter().any(|file| file.path == "guide.markdown"));
        assert!(recent.iter().all(|file| file.path != "plain.txt"));
        assert!(recent.iter().all(|file| file.path != "main.rs"));
        assert!(recent.iter().all(|file| file.name != "pic.png"));
        assert!(recent.iter().all(|file| file.name != "pkg.js"));
        assert!(recent.iter().all(|file| file.name != "empty-dir"));
        assert_eq!(recent.len(), 2);

        let index = index_in_project(&root).unwrap();
        assert!(index
            .files
            .iter()
            .all(|file| !file.path.contains("node_modules")));
        assert!(!index.truncated);
        assert!(index.files.iter().any(|file| file.path == "docs/notes.md"));
        assert!(index.files.iter().any(|file| file.path == "plain.txt"));
        assert!(index.files.iter().any(|file| file.name == "pic.png"));

        let listed = list_in_project(&root, "").unwrap();
        assert!(listed.entries.iter().any(|entry| entry.name == "plain.txt"));
        assert!(listed.entries.iter().any(|entry| entry.name == "pic.png"));
        assert!(listed.entries.iter().any(|entry| entry.name == "empty-dir" && entry.is_dir));

        // Newer non-previewable files must not push markdown out of Recents,
        // and must not appear there when opened or touched on disk.
        for index in 0..20 {
            fs::write(root.join(format!("newer-{index}.bin")), [0_u8, 1, 2]).unwrap();
            fs::write(root.join(format!("newer-{index}.txt")), "touch").unwrap();
        }
        let after_touch = recent_in_project(&root).unwrap();
        assert_eq!(after_touch.len(), 2);
        assert!(after_touch.iter().all(|file| file.kind == ProjectFileKind::Markdown));
        assert!(after_touch.iter().any(|file| file.path == "docs/notes.md"));
        assert!(after_touch
            .iter()
            .all(|file| !file.name.starts_with("newer-")));

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
        let listed = list_in_project(&real, "").unwrap();
        assert!(listed.entries.iter().all(|entry| entry.name != "leak"));
        assert!(list_in_project(&real, "leak").is_err());

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
        symlink(&outside, root.join("leak-dir")).unwrap();
        assert!(read_in_project(&root, "leak.txt").is_err());
        let listed = list_in_project(&root, "").unwrap();
        assert!(listed
            .entries
            .iter()
            .all(|entry| entry.name != "leak.txt" && entry.name != "leak-dir"));
        assert!(list_in_project(&root, "leak-dir").is_err());
        let indexed = index_in_project(&root).unwrap();
        assert!(indexed.files.iter().all(|file| file.name != "secret.txt"));
        let recent = recent_in_project(&root).unwrap();
        assert!(recent.iter().all(|file| !file.preview.contains("nope")));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }
}
