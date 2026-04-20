//! Env-var naming convention for services.
//!
//! Given a service name, produce the canonical env var name projects
//! should use for its port. Defined in PRD §9:
//!   - uppercase
//!   - hyphens become underscores
//!   - append `_PORT` (guard: don't double-append if name already ends in `port`)

pub fn env_var_name(service: &str) -> String {
    let normalised: String = service
        .chars()
        .map(|c| match c {
            '-' => '_',
            c => c.to_ascii_uppercase(),
        })
        .collect();

    if normalised.ends_with("_PORT") || normalised == "PORT" {
        normalised
    } else {
        format!("{normalised}_PORT")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_becomes_postgres_port() {
        assert_eq!(env_var_name("postgres"), "POSTGRES_PORT");
    }

    #[test]
    fn mailpit_smtp_becomes_mailpit_smtp_port() {
        assert_eq!(env_var_name("mailpit-smtp"), "MAILPIT_SMTP_PORT");
    }

    #[test]
    fn worker_2_becomes_worker_2_port() {
        assert_eq!(env_var_name("worker-2"), "WORKER_2_PORT");
    }

    #[test]
    fn already_uppercase_is_untouched_apart_from_suffix() {
        assert_eq!(env_var_name("REDIS"), "REDIS_PORT");
    }

    #[test]
    fn already_has_port_suffix_is_not_double_appended() {
        assert_eq!(env_var_name("my-port"), "MY_PORT");
    }

    #[test]
    fn already_has_uppercase_port_suffix_is_not_double_appended() {
        assert_eq!(env_var_name("MY_PORT"), "MY_PORT");
    }

    #[test]
    fn bare_port_is_not_double_appended() {
        assert_eq!(env_var_name("port"), "PORT");
    }

    #[test]
    fn multiple_hyphens() {
        assert_eq!(env_var_name("a-b-c"), "A_B_C_PORT");
    }

    #[test]
    fn numeric_service_name() {
        assert_eq!(env_var_name("123"), "123_PORT");
    }

    #[test]
    fn empty_string_gets_port_suffix() {
        // Edge case — empty service name. Shouldn't happen in practice
        // (project.rs validates) but the fn itself shouldn't panic.
        assert_eq!(env_var_name(""), "_PORT");
    }
}
