//! Registry file I/O: JSON load, atomic write, fd-lock advisory locking.
//!
//! The only public mutation path is `with_lock`: acquire advisory lock →
//! read → mutate via closure → atomic write → release. Callers cannot
//! hold the lock themselves.
//!
//! Reads via `read` are lock-free — safe because writes are atomic
//! (tempfile + rename).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::SpoutError;

use super::{Registry, CURRENT_VERSION};

const SPOUT_REGISTRY_ENV: &str = "SPOUT_REGISTRY";

pub fn registry_path() -> Result<PathBuf, SpoutError> {
    if let Ok(path) = std::env::var(SPOUT_REGISTRY_ENV) {
        return Ok(PathBuf::from(path));
    }
    dirs::home_dir()
        .map(|h| h.join(".spout.json"))
        .ok_or_else(|| SpoutError::RegistryCorrupt("cannot determine home directory".to_owned()))
}

/// `/tmp/foo.json` → `/tmp/foo.lock`. Critical for test isolation — every
/// test using a unique SPOUT_REGISTRY gets a unique lock file.
pub fn lock_path(registry: &Path) -> PathBuf {
    let mut p = registry.to_path_buf();
    p.set_extension("lock");
    p
}

pub fn read(path: &Path) -> Result<Registry, SpoutError> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let registry: Registry = serde_json::from_str(&contents)
                .map_err(|e| SpoutError::RegistryCorrupt(format!("parse failed: {e}")))?;
            if registry.version != 1 && registry.version != CURRENT_VERSION {
                return Err(SpoutError::RegistryVersionUnknown(registry.version));
            }
            Ok(registry)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(SpoutError::RegistryCorrupt(format!("read failed: {e}"))),
    }
}

pub fn write(path: &Path, registry: &Registry) -> Result<(), SpoutError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|e| SpoutError::RegistryCorrupt(format!("create parent: {e}")))?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| SpoutError::RegistryCorrupt(format!("tempfile: {e}")))?;

    let json = serde_json::to_string_pretty(registry)
        .map_err(|e| SpoutError::RegistryCorrupt(format!("serialise: {e}")))?;

    tmp.write_all(json.as_bytes())
        .map_err(|e| SpoutError::RegistryCorrupt(format!("write: {e}")))?;
    tmp.flush()
        .map_err(|e| SpoutError::RegistryCorrupt(format!("flush: {e}")))?;

    tmp.persist(path)
        .map_err(|e| SpoutError::RegistryCorrupt(format!("rename: {e}")))?;

    // The registry records full project identities (git remotes, absolute
    // CWD paths). On shared dev hosts the default 0644 umask would let
    // every other user enumerate it; force 0600 explicitly after persist.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| SpoutError::RegistryCorrupt(format!("set permissions: {e}")))?;
    }
    Ok(())
}

pub fn with_lock<F, T>(registry: &Path, f: F) -> Result<T, SpoutError>
where
    F: FnOnce(&mut Registry) -> Result<T, SpoutError>,
{
    let lock = lock_path(registry);
    if let Some(parent) = lock.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| SpoutError::RegistryCorrupt(format!("create lock parent: {e}")))?;
    }

    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)
        .map_err(|e| SpoutError::RegistryCorrupt(format!("open lock: {e}")))?;

    let mut rw = fd_lock::RwLock::new(file);
    let _guard = rw
        .write()
        .map_err(|e| SpoutError::RegistryCorrupt(format!("acquire lock: {e}")))?;

    let mut r = read(registry)?;
    r.version = CURRENT_VERSION;
    let result = f(&mut r)?;
    write(registry, &r)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Protocol;
    use tempfile::TempDir;

    fn temp_registry() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("spout.json");
        (dir, path)
    }

    #[test]
    fn read_missing_file_returns_empty_registry() {
        let (_dir, path) = temp_registry();
        let r = read(&path).unwrap();
        assert_eq!(r.version, CURRENT_VERSION);
        assert!(r.projects.is_empty());
        assert!(r.history.is_empty());
    }

    #[test]
    fn read_corrupt_json_exits_three() {
        let (_dir, path) = temp_registry();
        fs::write(&path, "{ not valid json").unwrap();
        let err = read(&path).unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn read_unknown_version_exits_four() {
        let (_dir, path) = temp_registry();
        fs::write(&path, r#"{"version":99,"projects":{}}"#).unwrap();
        let err = read(&path).unwrap_err();
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn write_then_read_round_trip() {
        let (_dir, path) = temp_registry();
        let mut r = Registry::default();
        r.set("myproj", "postgres", 19456, Protocol::default());
        write(&path, &r).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back.get("myproj", "postgres"), Some(19456));
    }

    #[test]
    fn with_lock_applies_mutation() {
        let (_dir, path) = temp_registry();
        with_lock(&path, |r| {
            r.set("myproj", "postgres", 19456, Protocol::default());
            Ok(())
        })
        .unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back.get("myproj", "postgres"), Some(19456));
    }

    #[test]
    fn with_lock_propagates_error_from_closure() {
        let (_dir, path) = temp_registry();
        let err = with_lock(&path, |_| Err::<(), _>(SpoutError::ServiceNotRegistered)).unwrap_err();
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn registry_path_resolves_to_an_absolute_path() {
        let path = registry_path().unwrap();
        assert!(path.is_absolute(), "got {path:?}");
        assert!(path.file_name().is_some());
    }

    #[test]
    fn lock_path_replaces_extension() {
        assert_eq!(
            lock_path(Path::new("/tmp/foo.json")),
            PathBuf::from("/tmp/foo.lock")
        );
        assert_eq!(
            lock_path(Path::new("/home/user/.spout.json")),
            PathBuf::from("/home/user/.spout.lock")
        );
    }

    #[test]
    fn concurrent_with_lock_serialises_writes() {
        use std::sync::Arc;
        use std::thread;
        let (dir, path) = temp_registry();
        let path = Arc::new(path);
        let _keep = Arc::new(dir);

        let mut handles = vec![];
        for i in 0..10u16 {
            let path = Arc::clone(&path);
            let keep = Arc::clone(&_keep);
            handles.push(thread::spawn(move || {
                let _ = &keep;
                with_lock(&path, |r| {
                    r.set(
                        "myproj",
                        &format!("svc{i}"),
                        20_000 + i,
                        Protocol::default(),
                    );
                    Ok(())
                })
                .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let back = read(&path).unwrap();
        let svcs = back.projects.get("myproj").unwrap();
        assert_eq!(svcs.len(), 10);
    }
}
