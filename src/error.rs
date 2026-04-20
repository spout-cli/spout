//! `SpoutError` — every failure mode maps to exactly one exit code.
//!
//! Exit codes are part of the CLI's stable API. See README.md.

use thiserror::Error;

#[derive(Debug, Error)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum SpoutError {
    #[error("service not registered")]
    ServiceNotRegistered,

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

    #[error("port {port} is already registered to project '{project}'")]
    PortAlreadyClaimed { port: u16, project: String },

    #[error("port {0} is already in use by the operating system")]
    PortInUse(u16),
}

impl SpoutError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::ServiceNotRegistered => 1,
            Self::NoFreePortFound { .. } => 2,
            Self::RegistryCorrupt(_) => 3,
            Self::RegistryVersionUnknown(_) => 4,
            Self::PortAlreadyClaimed { .. } => 5,
            Self::PortInUse(_) => 6,
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
            project: "other".into(),
        };
        assert_eq!(err.exit_code(), 5);
    }

    #[test]
    fn port_in_use_exits_six() {
        assert_eq!(SpoutError::PortInUse(5432).exit_code(), 6);
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
                project: "tyfi".into(),
            },
            SpoutError::PortInUse(6379),
        ];
        for v in &variants {
            assert!(!v.to_string().is_empty());
        }
    }
}
