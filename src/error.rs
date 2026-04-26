//! `SpoutError` — every failure mode maps to exactly one exit code.
//!
//! Exit codes are part of the CLI's stable API. See README.md.

use thiserror::Error;

use crate::protocol::Protocol;

#[derive(Debug, Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum SpoutError {
    #[error("service not registered")]
    ServiceNotRegistered,

    #[error("{}", format_not_registered_help(.project, .service, .available, .recently_removed.as_ref()))]
    ServiceNotRegisteredInProject {
        project: String,
        service: String,
        available: Vec<String>,
        recently_removed: Option<RemovedRecord>,
    },

    #[error("no free port found for {service} in range {range_start}-{range_end}")]
    NoFreePortFound {
        service: String,
        range_start: u16,
        range_end: u16,
    },

    #[error("registry unreadable: {0}")]
    RegistryCorrupt(String),

    #[error("registry version {0} is not supported")]
    RegistryVersionUnknown(u32),

    #[error("port {port}/{protocol} is already registered to project '{project}'")]
    PortAlreadyClaimed {
        port: u16,
        protocol: Protocol,
        project: String,
    },

    #[error("port {port}/{protocol} is already in use by the operating system")]
    PortInUse { port: u16, protocol: Protocol },

    #[error("I/O error: {0}")]
    Io(String),

    #[error("compose file unreadable: {0}")]
    ComposeInvalid(String),

    #[error("{0}")]
    ComposeNotFound(String),

    #[error("{0}")]
    Usage(String),
}

/// Snapshot of a service's most recent removal record. Independent of
/// `registry::HistoryEntry` so `error.rs` doesn't depend on registry
/// types — the mapping happens at the error-construction site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedRecord {
    pub released: String,
    pub reason: String,
}

fn format_not_registered_help(
    project: &str,
    service: &str,
    available: &[String],
    recently_removed: Option<&RemovedRecord>,
) -> String {
    let mut lines = vec![format!("no service '{service}' in project '{project}'")];
    if available.is_empty() {
        lines.push("  no services currently registered".to_string());
    } else {
        lines.push(format!("  available: {}", available.join(", ")));
    }
    if let Some(r) = recently_removed {
        lines.push(format!(
            "  recently removed: {service} ({}, \"{}\")",
            r.released, r.reason
        ));
    }
    let hint = match (available.is_empty(), recently_removed.is_some()) {
        (true, false) => format!("  (try `spout alloc {service}`)"),
        (true, true) => format!("  (try `spout alloc {service}` to register fresh)"),
        (false, _) => "  (try `spout env` for KEY=VALUE)".to_string(),
    };
    lines.push(hint);
    lines.join("\n")
}

impl SpoutError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::ServiceNotRegistered => 1,
            Self::ServiceNotRegisteredInProject { .. } => 1,
            Self::NoFreePortFound { .. } => 2,
            Self::RegistryCorrupt(_) => 3,
            Self::RegistryVersionUnknown(_) => 4,
            Self::PortAlreadyClaimed { .. } => 5,
            Self::PortInUse { .. } => 6,
            Self::Io(_) => 7,
            Self::ComposeInvalid(_) => 8,
            Self::ComposeNotFound(_) => 8,
            Self::Usage(_) => 9,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_not_registered_exits_one() {
        assert_eq!(SpoutError::ServiceNotRegistered.exit_code(), 1);
    }

    #[test]
    fn no_free_port_found_exits_two() {
        let err = SpoutError::NoFreePortFound {
            service: "postgres".into(),
            range_start: 5432,
            range_end: 6432,
        };
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn registry_corrupt_exits_three() {
        let err = SpoutError::RegistryCorrupt("bad json".into());
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn registry_version_unknown_exits_four() {
        assert_eq!(SpoutError::RegistryVersionUnknown(2).exit_code(), 4);
    }

    #[test]
    fn port_already_claimed_exits_five() {
        let err = SpoutError::PortAlreadyClaimed {
            port: 5432,
            protocol: Protocol::Tcp,
            project: "other".into(),
        };
        assert_eq!(err.exit_code(), 5);
    }

    #[test]
    fn port_in_use_exits_six() {
        let err = SpoutError::PortInUse {
            port: 5432,
            protocol: Protocol::Tcp,
        };
        assert_eq!(err.exit_code(), 6);
    }

    #[test]
    fn io_exits_seven() {
        let err = SpoutError::Io("broken pipe".into());
        assert_eq!(err.exit_code(), 7);
    }

    #[test]
    fn compose_invalid_exits_eight() {
        let err = SpoutError::ComposeInvalid("bad yaml".into());
        assert_eq!(err.exit_code(), 8);
    }

    #[test]
    fn compose_not_found_exits_eight() {
        assert_eq!(
            SpoutError::ComposeNotFound("no compose file".into()).exit_code(),
            8
        );
    }

    #[test]
    fn usage_exits_nine() {
        let err = SpoutError::Usage("specify a service".into());
        assert_eq!(err.exit_code(), 9);
    }

    #[test]
    fn display_messages_are_non_empty() {
        let variants = [
            SpoutError::ServiceNotRegistered,
            SpoutError::NoFreePortFound {
                service: "api".into(),
                range_start: 8080,
                range_end: 9080,
            },
            SpoutError::RegistryCorrupt("expected `{`".into()),
            SpoutError::RegistryVersionUnknown(99),
            SpoutError::PortAlreadyClaimed {
                port: 5432,
                protocol: Protocol::Tcp,
                project: "myapp".into(),
            },
            SpoutError::PortInUse {
                port: 6379,
                protocol: Protocol::Udp,
            },
        ];
        for v in &variants {
            assert!(!v.to_string().is_empty());
        }
    }

    #[test]
    fn port_errors_mention_the_protocol() {
        let claimed = SpoutError::PortAlreadyClaimed {
            port: 5432,
            protocol: Protocol::Udp,
            project: "p".into(),
        };
        assert!(claimed.to_string().contains("5432/udp"), "got {claimed}");

        let in_use = SpoutError::PortInUse {
            port: 53,
            protocol: Protocol::Udp,
        };
        assert!(in_use.to_string().contains("53/udp"), "got {in_use}");
    }
}
