//! Project identity inference.
//!
//! Layered — first match wins:
//! 0. `SPOUT_PROJECT` env var, trimmed. Escape hatch for monorepos and
//!    anywhere the git-remote heuristic gives the wrong answer.
//! 1. `git config --get remote.origin.url`, parsed to `host/owner/repo`.
//!    Stable across filesystem moves.
//! 2. `git rev-parse --show-toplevel` — git root absolute path. Used when
//!    the repo has no remote.
//! 3. Absolute CWD — when there's no git at all, or git isn't installed.
//!
//! Resolved identity is cached in a `OnceLock` — the two `git` shell-outs
//! together cost ~60-100ms cold and would otherwise be paid on every
//! single spout invocation.

use std::env;
use std::process::Command;
use std::sync::OnceLock;

use crate::error::SpoutError;

const SPOUT_PROJECT_ENV: &str = "SPOUT_PROJECT";

pub fn current_project() -> Result<String, SpoutError> {
    static CACHE: OnceLock<String> = OnceLock::new();
    if let Some(cached) = CACHE.get() {
        return Ok(cached.clone());
    }
    let resolved = resolve()?;
    let _ = CACHE.set(resolved.clone());
    Ok(resolved)
}

fn resolve() -> Result<String, SpoutError> {
    resolve_with_override(env::var(SPOUT_PROJECT_ENV).ok())
}

fn resolve_with_override(override_value: Option<String>) -> Result<String, SpoutError> {
    if let Some(explicit) = override_value.and_then(non_empty_trimmed) {
        return Ok(explicit);
    }
    if let Some(identity) = git_remote_identity() {
        return Ok(identity);
    }
    if let Some(path) = git_root_path() {
        return Ok(path);
    }
    cwd_path()
}

fn non_empty_trimmed(raw: String) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn git_remote_identity() -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8(output.stdout).ok()?;
    parse_remote_url(url.trim())
}

fn git_root_path() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn cwd_path() -> Result<String, SpoutError> {
    let cwd = env::current_dir()
        .map_err(|e| SpoutError::RegistryCorrupt(format!("cannot read current directory: {e}")))?;
    cwd.to_str()
        .map(|s| s.to_owned())
        .ok_or_else(|| SpoutError::RegistryCorrupt("CWD contains non-UTF-8 bytes".to_owned()))
}

/// Parse a git remote URL into `host/owner/repo` form.
///
/// Handles the forms git emits via `remote.origin.url`:
/// - SCP-like: `git@github.com:org/repo.git`, `git@host:org/repo`
/// - HTTPS: `https://github.com/org/repo.git`, `https://host/org/repo`
/// - SSH: `ssh://git@github.com/org/repo.git`
/// - file/local paths pass through unchanged.
fn parse_remote_url(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }

    // Strip leading protocol.
    let s = strip_protocol(url);

    // Strip leading `user@` if it precedes the host.
    let s = strip_user(s);

    // Normalise SCP-like `host:path` to `host/path`.
    let normalised = normalise_scp_like(s);

    // Strip trailing `.git`.
    let clean = normalised
        .strip_suffix(".git")
        .unwrap_or(&normalised)
        .to_owned();

    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}

fn strip_protocol(s: &str) -> &str {
    for prefix in ["ssh://", "git://", "https://", "http://", "file://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return rest;
        }
    }
    s
}

fn strip_user(s: &str) -> &str {
    let Some(at_pos) = s.find('@') else {
        return s;
    };
    // Only strip if @ comes before the first `:` or `/` — otherwise the @
    // might be part of the path (rare but technically legal).
    let boundary = s.find([':', '/']);
    match boundary {
        Some(b) if at_pos < b => &s[at_pos + 1..],
        _ => s,
    }
}

fn normalise_scp_like(s: &str) -> String {
    let colon = s.find(':');
    let slash = s.find('/');
    match (colon, slash) {
        (Some(c), Some(sl)) if c < sl => format!("{}/{}", &s[..c], &s[c + 1..]),
        (Some(c), None) => format!("{}/{}", &s[..c], &s[c + 1..]),
        _ => s.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scp_like_with_user() {
        assert_eq!(
            parse_remote_url("git@github.com:spout-cli/spout.git").unwrap(),
            "github.com/spout-cli/spout"
        );
    }

    #[test]
    fn parse_scp_like_without_dotgit() {
        assert_eq!(
            parse_remote_url("git@github.com:spout-cli/spout").unwrap(),
            "github.com/spout-cli/spout"
        );
    }

    #[test]
    fn parse_https() {
        assert_eq!(
            parse_remote_url("https://github.com/spout-cli/spout.git").unwrap(),
            "github.com/spout-cli/spout"
        );
    }

    #[test]
    fn parse_https_without_dotgit() {
        assert_eq!(
            parse_remote_url("https://github.com/spout-cli/spout").unwrap(),
            "github.com/spout-cli/spout"
        );
    }

    #[test]
    fn parse_ssh_with_protocol() {
        assert_eq!(
            parse_remote_url("ssh://git@github.com/spout-cli/spout.git").unwrap(),
            "github.com/spout-cli/spout"
        );
    }

    #[test]
    fn parse_gitlab_multi_group() {
        assert_eq!(
            parse_remote_url("git@gitlab.internal.co:team/subteam/project.git").unwrap(),
            "gitlab.internal.co/team/subteam/project"
        );
    }

    #[test]
    fn parse_http_no_user() {
        assert_eq!(
            parse_remote_url("http://git.example.com/foo/bar.git").unwrap(),
            "git.example.com/foo/bar"
        );
    }

    #[test]
    fn parse_scp_like_without_user() {
        assert_eq!(
            parse_remote_url("github.com:org/repo.git").unwrap(),
            "github.com/org/repo"
        );
    }

    #[test]
    fn parse_empty_returns_none() {
        assert!(parse_remote_url("").is_none());
    }

    #[test]
    fn parse_local_path_passes_through() {
        // A local file-backed remote — weird but valid. We don't try to
        // pretty-print it; just keep the path as the identity.
        let parsed = parse_remote_url("/home/user/other/repo.git").unwrap();
        assert_eq!(parsed, "/home/user/other/repo");
    }

    #[test]
    fn current_project_returns_something_non_empty() {
        // Sanity check: in this crate's own dir (git repo with an origin),
        // we should get a non-empty identity. The exact value depends on
        // whether an origin is configured, and the CI environment.
        let id = current_project().unwrap();
        assert!(!id.is_empty(), "current_project() returned empty string");
    }

    #[test]
    fn resolve_with_override_honours_explicit_name() {
        let id = resolve_with_override(Some("my-monorepo/web".to_owned())).unwrap();
        assert_eq!(id, "my-monorepo/web");
    }

    #[test]
    fn resolve_with_override_trims_whitespace() {
        let id = resolve_with_override(Some("  custom  ".to_owned())).unwrap();
        assert_eq!(id, "custom");
    }

    #[test]
    fn resolve_with_override_falls_through_on_empty_string() {
        let id = resolve_with_override(Some(String::new())).unwrap();
        // Falls through to git/CWD logic — value depends on env but must be non-empty.
        assert!(!id.is_empty());
    }

    #[test]
    fn resolve_with_override_falls_through_on_whitespace_only() {
        let id = resolve_with_override(Some("   ".to_owned())).unwrap();
        assert!(!id.is_empty());
    }

    #[test]
    fn resolve_with_override_falls_through_on_none() {
        let id = resolve_with_override(None).unwrap();
        assert!(!id.is_empty());
    }
}
