//! Project name inference.
//!
//! Matches Docker Compose's convention: the project is the final component
//! of the current working directory. Monorepo handling (walk to git root)
//! is deliberately out of scope for v1 — see `docs/spout-prd.md`.

#![cfg_attr(not(test), allow(dead_code))]

use std::env;
use std::path::Path;

use crate::error::SpoutError;

pub fn current_project() -> Result<String, SpoutError> {
    let cwd = env::current_dir()
        .map_err(|e| SpoutError::RegistryCorrupt(format!("cannot read current directory: {e}")))?;
    project_from_path(&cwd)
}

fn project_from_path(path: &Path) -> Result<String, SpoutError> {
    path.file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            SpoutError::RegistryCorrupt(format!(
                "cannot determine project name from path: {}",
                path.display()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn returns_final_component_for_normal_path() {
        let p = PathBuf::from("/home/user/dev/rust/spout");
        assert_eq!(project_from_path(&p).unwrap(), "spout");
    }

    #[test]
    fn returns_final_component_for_single_segment() {
        let p = PathBuf::from("/acme");
        assert_eq!(project_from_path(&p).unwrap(), "acme");
    }

    #[test]
    fn strips_trailing_slash() {
        let p = PathBuf::from("/home/user/myproj/");
        assert_eq!(project_from_path(&p).unwrap(), "myproj");
    }

    #[test]
    fn handles_unicode_directory_names() {
        let p = PathBuf::from("/home/user/проект");
        assert_eq!(project_from_path(&p).unwrap(), "проект");
    }

    #[test]
    fn errors_on_root_path() {
        let err = project_from_path(Path::new("/")).unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn errors_on_empty_path() {
        let err = project_from_path(Path::new("")).unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn errors_on_dot_path() {
        let err = project_from_path(Path::new(".")).unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn current_project_returns_crate_directory_name() {
        let name = current_project().unwrap();
        assert_eq!(name, "spout");
    }
}
