//! Compose-marker walk used by `project::resolve_with_override` to detect
//! monorepo subprojects.
//!
//! Pure functions on `&Path` — the impure side (reading CWD and git root)
//! lives in `project.rs`. Keeps this module free of `OnceLock`, `env`, and
//! `Command` dependencies so it's testable purely from temp directories.

use std::path::Path;

/// Marker filenames recognised by Docker Compose itself. Narrow on purpose —
/// language-specific markers (Cargo.toml, package.json, etc.) invite false
/// positives in workspace/monorepo setups where nested manifests aren't
/// independent projects. See docs/planning/03-planning.md §3b for rationale.
const COMPOSE_MARKERS: &[&str] = &[
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
];

/// Walk up from `cwd` looking for the nearest ancestor directory (strictly
/// below `git_root`) that contains a compose marker. Return its path relative
/// to `git_root` in POSIX form, or `None` if no such ancestor exists.
///
/// A marker found at `git_root` itself returns `None` — it adds no information
/// beyond the git-remote identity, and including it would change today's
/// behavior for single-project repos with a root-level compose file.
///
/// Both paths are canonicalised before comparison to handle symlinked temp
/// directories on macOS and any `..` components.
pub fn compose_marker_subdir(git_root: &Path, cwd: &Path) -> Option<String> {
    let git_root = git_root.canonicalize().ok()?;
    let cwd = cwd.canonicalize().ok()?;
    if !cwd.starts_with(&git_root) {
        return None;
    }
    let mut cursor = cwd;
    loop {
        if cursor == git_root {
            return None;
        }
        if has_compose_marker(&cursor) {
            let relative = cursor.strip_prefix(&git_root).ok()?;
            return path_to_posix(relative);
        }
        let parent = cursor.parent()?;
        cursor = parent.to_path_buf();
    }
}

fn has_compose_marker(dir: &Path) -> bool {
    COMPOSE_MARKERS.iter().any(|name| dir.join(name).is_file())
}

/// Join path components with `/`. Returns `None` if any component isn't
/// valid UTF-8 — callers fall back to a non-marker identity rather than
/// build a silently-truncated subdir (which could alias a real directory
/// with fewer components).
fn path_to_posix(p: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = p.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn canonical(p: &Path) -> PathBuf {
        p.canonicalize().unwrap()
    }

    fn touch(path: &Path) {
        fs::write(path, b"").unwrap();
    }

    #[test]
    fn marker_subdir_returns_none_when_no_marker() {
        let root = TempDir::new().unwrap();
        let cwd = root.path().join("apps").join("web");
        fs::create_dir_all(&cwd).unwrap();
        assert!(compose_marker_subdir(&canonical(root.path()), &canonical(&cwd)).is_none());
    }

    #[test]
    fn marker_subdir_returns_none_when_marker_only_at_git_root() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join("docker-compose.yml"));
        let cwd = root.path().join("apps").join("web");
        fs::create_dir_all(&cwd).unwrap();
        // Root marker adds no information beyond the git-remote identity.
        assert!(compose_marker_subdir(&canonical(root.path()), &canonical(&cwd)).is_none());
    }

    #[test]
    fn marker_subdir_returns_none_when_cwd_is_root_with_marker() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join("docker-compose.yml"));
        assert!(compose_marker_subdir(&canonical(root.path()), &canonical(root.path())).is_none());
    }

    #[test]
    fn marker_subdir_returns_relative_path_when_marker_on_cwd() {
        let root = TempDir::new().unwrap();
        let web = root.path().join("apps").join("web");
        fs::create_dir_all(&web).unwrap();
        touch(&web.join("docker-compose.yml"));
        let subdir = compose_marker_subdir(&canonical(root.path()), &canonical(&web)).unwrap();
        assert_eq!(subdir, "apps/web");
    }

    #[test]
    fn marker_subdir_walks_up_from_deeper_cwd() {
        let root = TempDir::new().unwrap();
        let web = root.path().join("apps").join("web");
        let deeper = web.join("cmd").join("server");
        fs::create_dir_all(&deeper).unwrap();
        touch(&web.join("docker-compose.yml"));
        let subdir = compose_marker_subdir(&canonical(root.path()), &canonical(&deeper)).unwrap();
        assert_eq!(subdir, "apps/web");
    }

    #[test]
    fn marker_subdir_nearest_wins_over_root_marker() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join("docker-compose.yml"));
        let web = root.path().join("apps").join("web");
        let cmd = web.join("cmd");
        fs::create_dir_all(&cmd).unwrap();
        touch(&web.join("docker-compose.yml"));
        let subdir = compose_marker_subdir(&canonical(root.path()), &canonical(&cmd)).unwrap();
        assert_eq!(subdir, "apps/web");
    }

    #[test]
    fn marker_subdir_recognises_all_four_filenames() {
        for name in [
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yml",
            "compose.yaml",
        ] {
            let root = TempDir::new().unwrap();
            let svc = root.path().join("svc");
            fs::create_dir(&svc).unwrap();
            touch(&svc.join(name));
            let subdir = compose_marker_subdir(&canonical(root.path()), &canonical(&svc));
            assert_eq!(subdir.as_deref(), Some("svc"), "failed for marker {name}");
        }
    }

    #[test]
    fn marker_subdir_ignores_directory_named_like_marker() {
        let root = TempDir::new().unwrap();
        let svc = root.path().join("svc");
        fs::create_dir_all(svc.join("docker-compose.yml")).unwrap();
        // A directory (not a file) named docker-compose.yml must NOT count as a marker.
        assert!(compose_marker_subdir(&canonical(root.path()), &canonical(&svc)).is_none());
    }
}
