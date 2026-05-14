//! Service-name helpers used by the TUI.
//!
//! - `env_var_name`: canonical `SERVICE_PORT`-style env var name for a
//!   service. Surfaced in the TUI so users can copy it.
//! - `service_icon`: optional user-provided icon for a service, sourced
//!   from the `SPOUT_ICONS` env var. Spout itself ships no mapping — the
//!   onus is on the user to define one (e.g. in their shell rc).

use std::collections::HashMap;
use std::sync::OnceLock;

const SPOUT_ICONS_ENV: &str = "SPOUT_ICONS";

/// Canonical env-var name for a service's port.
///
/// Uppercases the service name, replaces any non-alphanumeric character
/// with an underscore, and appends `_PORT` unless the result already
/// ends in `_PORT` or is bare `PORT`. The non-alphanumeric sanitisation
/// matters for shell safety — the README's `eval $(spout env)` pattern
/// would otherwise evaluate any `$()` / `;` / backticks in service
/// names as shell, turning a crafted compose file into an RCE chain.
pub fn env_var_name(service: &str) -> String {
    let normalised: String = service
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' => c.to_ascii_uppercase(),
            _ => '_',
        })
        .collect();

    if normalised.ends_with("_PORT") || normalised == "PORT" {
        normalised
    } else {
        format!("{normalised}_PORT")
    }
}

/// Look up the user-defined icon for `service`, if any.
///
/// Reads `SPOUT_ICONS` once per process (env can't change mid-run) and
/// caches the parsed map. Unset, empty, or all-malformed env → always
/// returns `None`. The returned reference lives as long as the process;
/// callers never need to clone.
pub fn service_icon(service: &str) -> Option<&'static str> {
    icons_map().get(service).map(String::as_str)
}

fn icons_map() -> &'static HashMap<String, String> {
    static CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();
    CACHE.get_or_init(|| parse_icons(&std::env::var(SPOUT_ICONS_ENV).unwrap_or_default()))
}

/// Parse a `service=icon,service=icon` string into a map.
///
/// Whitespace around keys and values is trimmed. Entries with no `=`,
/// empty key, or empty value are skipped with a `tracing::warn!` so
/// they surface under `-v`. Everything else is tolerated — trailing
/// commas, repeated commas, etc.
fn parse_icons(src: &str) -> HashMap<String, String> {
    src.split(',')
        .filter_map(|pair| {
            let pair = pair.trim();
            if pair.is_empty() {
                return None;
            }
            let Some((k, v)) = pair.split_once('=') else {
                tracing::warn!("SPOUT_ICONS entry missing '=': {pair:?}");
                return None;
            };
            let k = k.trim();
            let v = v.trim();
            if k.is_empty() || v.is_empty() {
                tracing::warn!("SPOUT_ICONS entry has empty key or value: {pair:?}");
                return None;
            }
            Some((k.to_owned(), v.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_name_postgres_becomes_postgres_port() {
        assert_eq!(env_var_name("postgres"), "POSTGRES_PORT");
    }

    #[test]
    fn env_var_name_hyphen_becomes_underscore() {
        assert_eq!(env_var_name("mailpit-smtp"), "MAILPIT_SMTP_PORT");
    }

    #[test]
    fn env_var_name_preserves_digits() {
        assert_eq!(env_var_name("worker-2"), "WORKER_2_PORT");
    }

    #[test]
    fn env_var_name_already_uppercase_only_appends_suffix() {
        assert_eq!(env_var_name("REDIS"), "REDIS_PORT");
    }

    #[test]
    fn env_var_name_lowercase_port_suffix_is_not_double_appended() {
        assert_eq!(env_var_name("my-port"), "MY_PORT");
    }

    #[test]
    fn env_var_name_uppercase_port_suffix_is_not_double_appended() {
        assert_eq!(env_var_name("MY_PORT"), "MY_PORT");
    }

    #[test]
    fn env_var_name_bare_port_is_not_double_appended() {
        assert_eq!(env_var_name("port"), "PORT");
    }

    #[test]
    fn env_var_name_multiple_hyphens() {
        assert_eq!(env_var_name("a-b-c"), "A_B_C_PORT");
    }

    #[test]
    fn env_var_name_numeric_service_name() {
        assert_eq!(env_var_name("123"), "123_PORT");
    }

    #[test]
    fn env_var_name_empty_string_gets_port_suffix() {
        assert_eq!(env_var_name(""), "_PORT");
    }

    #[test]
    fn env_var_name_sanitises_shell_metachars() {
        // Crafted service names must not survive into env output —
        // `eval $(spout env)` would otherwise be RCE-able. Property:
        // no shell-active character appears in the result for any
        // input containing one.
        let hostile = [
            "foo$bar",
            "foo`bar`",
            "foo$(bar)",
            "foo;bar",
            "foo bar",
            "foo|bar",
            "foo&bar",
            "foo>bar",
        ];
        let bad = ['$', '`', '(', ')', ';', ' ', '|', '&', '>', '<', '"', '\''];
        for service in hostile {
            let out = env_var_name(service);
            for c in bad {
                assert!(
                    !out.contains(c),
                    "{out:?} from {service:?} still contains {c:?}"
                );
            }
        }
    }

    #[test]
    fn parse_icons_empty_string_is_empty_map() {
        assert!(parse_icons("").is_empty());
    }

    #[test]
    fn parse_icons_single_entry() {
        let map = parse_icons("postgres=🐘");
        assert_eq!(map.get("postgres"), Some(&"🐘".to_owned()));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn parse_icons_multiple_entries() {
        let map = parse_icons("postgres=🐘,redis=🔴,api=🌐");
        assert_eq!(map.get("postgres"), Some(&"🐘".to_owned()));
        assert_eq!(map.get("redis"), Some(&"🔴".to_owned()));
        assert_eq!(map.get("api"), Some(&"🌐".to_owned()));
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn parse_icons_trims_whitespace_around_keys_and_values() {
        let map = parse_icons("  postgres = 🐘 ,  redis=🔴  ");
        assert_eq!(map.get("postgres"), Some(&"🐘".to_owned()));
        assert_eq!(map.get("redis"), Some(&"🔴".to_owned()));
    }

    #[test]
    fn parse_icons_tolerates_trailing_and_repeated_commas() {
        let map = parse_icons(",postgres=🐘,,redis=🔴,");
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn parse_icons_skips_entry_with_no_equals() {
        let map = parse_icons("garbage,postgres=🐘");
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("postgres"), Some(&"🐘".to_owned()));
    }

    #[test]
    fn parse_icons_skips_empty_key() {
        let map = parse_icons("=noicon,postgres=🐘");
        assert_eq!(map.len(), 1);
        assert!(!map.contains_key(""));
    }

    #[test]
    fn parse_icons_skips_empty_value() {
        let map = parse_icons("postgres=,redis=🔴");
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("redis"), Some(&"🔴".to_owned()));
    }

    #[test]
    fn parse_icons_allows_equals_in_value() {
        // split_once on first '=' only — users could put key=value-ish
        // strings as icons if they ever wanted to, and we shouldn't
        // silently truncate them.
        let map = parse_icons("weird=a=b");
        assert_eq!(map.get("weird"), Some(&"a=b".to_owned()));
    }
}
