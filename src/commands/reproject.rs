//! `spout reproject` — move every registration from one project identity
//! to another. Used when a project's identity changes (e.g., `git init`
//! after services were registered under the cwd path).

use std::path::Path;

use crate::error::SpoutError;
use crate::registry;

pub fn run(registry_path: &Path, from: &str, to: &str) -> Result<String, SpoutError> {
    if from == to {
        return Err(SpoutError::Usage(
            "--from and --to must be different project identities".into(),
        ));
    }
    let moved = registry::with_lock(registry_path, |r| match r.reproject(from, to) {
        Ok(count) => Ok(count),
        Err(conflicts) => Err(SpoutError::ReprojectConflict {
            from: from.to_owned(),
            to: to.to_owned(),
            services: conflicts,
        }),
    })?;
    if moved == 0 {
        Ok(format!("no services registered under '{from}'"))
    } else {
        Ok(format!(
            "Moved {moved} service{} from '{from}' to '{to}'.",
            if moved == 1 { "" } else { "s" }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn run_refuses_when_from_equals_to() {
        // Empty path is fine — validation fails before we touch the registry.
        let result = run(&PathBuf::new(), "same", "same");
        match result {
            Err(SpoutError::Usage(msg)) => assert!(msg.contains("--from and --to must be")),
            other => panic!("expected Usage error, got {other:?}"),
        }
    }
}
